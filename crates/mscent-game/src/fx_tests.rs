//! Pools, emitters, the clock and hostile input. Collision has its own file.

use super::*;
use crate::test_util::{far_goal, game, open_level, random_trace, start, v};
use mscent_math::Aabb;

pub const fn class(collide: Collide, capacity: u16, tier: Tier) -> ParticleClass {
    ParticleClass {
        prim: Prim::Spark,
        blend: Blend::Add,
        tier,
        gravity: 0.0,
        drag: 0.0,
        collide,
        ramp: &[0xFFFF_FFFF],
        size: (0.1, 0.0),
        stretch: 0.0,
        capacity,
        trail: None,
    }
}

pub const fn emitter(class: u8, count: u16, shape: Shape, speed: f32, life: f32) -> Emitter {
    Emitter {
        class: ClassId(class),
        count: (count, count),
        shape,
        speed: (speed, speed),
        life: (life, life),
        delay: 0.0,
        tint: TintSource::Class,
    }
}

pub fn args_at(pos: Vec3) -> EffectArgs {
    EffectArgs {
        pos,
        dir: Vec3::ZERO,
        end: pos,
        scale: 1.0,
        surface: 0x80_40_20,
        volume: None,
    }
}

pub fn room_bsp() -> Bsp {
    let level = open_level(vec![start(v(0.0, 0.0, 0.0), 0.0), far_goal()]);
    game(&level).arena.bsp
}

/// Every particle as raw bits, so equality means bit for bit.
fn snapshot(fx: &Fx) -> Vec<[u32; 9]> {
    let mut out = Vec::new();
    fx.for_each(|p| {
        let b = |f: f32| f.to_bits();
        out.push([
            u32::from(p.class.0),
            b(p.pos.x),
            b(p.pos.y),
            b(p.pos.z),
            b(p.vel.x),
            b(p.vel.y),
            b(p.vel.z),
            b(p.t),
            u32::from(p.key) << 24 | p.surface,
        ]);
    });
    out
}

fn assert_finite(fx: &Fx) {
    fx.for_each(|p| {
        assert!(
            p.pos.is_finite() && p.vel.is_finite() && p.t.is_finite(),
            "{p:?}"
        );
    });
}

const MIXED: [ParticleClass; 3] = [
    class(Collide::None, 600, Tier::Normal),
    class(Collide::Die, 600, Tier::Normal),
    class(Collide::Bounce { keep: 0.5 }, 600, Tier::Normal),
];

static BLAST: [Emitter; 4] = [
    emitter(0, 200, Shape::Sphere(1.0), 8.0, 3.0),
    emitter(1, 200, Shape::Cone { half_angle: 0.5 }, 12.0, 3.0),
    emitter(2, 200, Shape::Hemisphere, 20.0, 3.0),
    Emitter {
        delay: 0.1,
        ..emitter(0, 50, Shape::Volume, 1.0, 2.0)
    },
];

fn run_blast(seed: u64) -> Fx {
    let bsp = room_bsp();
    let mut fx = Fx::new(&MIXED, seed);
    let mut a = args_at(v(0.0, 1.0, 0.0));
    a.dir = v(0.0, 1.0, 0.0);
    a.volume = Some(Aabb::new(v(-5.0, -5.0, -5.0), v(5.0, 5.0, 5.0)));
    fx.spawn(&BLAST, &a);
    for _ in 0..40 {
        fx.advance(50_000_000, &bsp);
    }
    fx
}

#[test]
fn same_seed_is_bit_identical_and_a_different_seed_differs() {
    let (a, b, c) = (run_blast(7), run_blast(7), run_blast(8));
    assert!(a.live() > 0);
    assert_eq!(snapshot(&a), snapshot(&b));
    assert_ne!(snapshot(&a), snapshot(&c));
}

#[test]
fn counts_are_exact_and_particles_start_where_asked() {
    let bsp = room_bsp();
    let mut fx = Fx::new(&MIXED, 1);
    static E: [Emitter; 1] = [emitter(0, 10, Shape::Point, 0.0, 5.0)];
    fx.spawn(&E, &args_at(v(3.0, 2.0, 1.0)));
    assert_eq!(fx.live(), 10);
    fx.for_each(|p| assert_eq!(p.pos, v(3.0, 2.0, 1.0)));
    fx.advance(0, &bsp);
    assert_eq!(fx.live(), 10);
    fx.clear();
    assert_eq!((fx.live(), fx.queued()), (0, 0));
}

#[test]
fn particles_expire_after_their_life() {
    let bsp = room_bsp();
    let mut fx = Fx::new(&MIXED, 1);
    static E: [Emitter; 1] = [emitter(0, 50, Shape::Point, 1.0, 0.5)];
    fx.spawn(&E, &args_at(Vec3::ZERO));
    // Short advances: one call never runs more than the catch-up cap.
    for _ in 0..8 {
        fx.advance(50_000_000, &bsp);
    }
    assert_eq!(fx.live(), 50);
    for _ in 0..2 {
        fx.advance(50_000_000, &bsp);
    }
    assert_eq!(fx.live(), 0);
}

#[test]
fn delayed_emitters_wait_then_fire_and_a_full_queue_drops() {
    let bsp = room_bsp();
    let mut fx = Fx::new(&MIXED, 1);
    static E: [Emitter; 1] = [Emitter {
        delay: 0.1,
        ..emitter(0, 3, Shape::Point, 0.0, 5.0)
    }];
    fx.spawn(&E, &args_at(Vec3::ZERO));
    assert_eq!((fx.live(), fx.queued()), (0, 1));
    fx.advance(50_000_000, &bsp);
    assert_eq!(fx.live(), 0);
    fx.advance(100_000_000, &bsp);
    assert_eq!((fx.live(), fx.queued()), (3, 0));

    for _ in 0..QUEUE_CAPACITY + 10 {
        fx.spawn(&E, &args_at(Vec3::ZERO));
    }
    assert_eq!(fx.queued(), QUEUE_CAPACITY);
}

#[test]
fn essential_replaces_the_oldest_and_normal_drops() {
    let bsp = room_bsp();
    static TABLE: [ParticleClass; 2] = [
        class(Collide::None, 4, Tier::Essential),
        class(Collide::None, 4, Tier::Normal),
    ];
    for (class_id, replaced) in [(0u8, true), (1u8, false)] {
        let mut fx = Fx::new(&TABLE, 1);
        // Born at x = 0, 1, 2, 3 with a tick between, so x = 0 is oldest.
        static E: [Emitter; 1] = [emitter(0, 1, Shape::Point, 0.0, 5.0)];
        let mut e = E[0];
        e.class = ClassId(class_id);
        let effect: &'static [Emitter] = Box::leak(Box::new([e]));
        for x in 0..4 {
            fx.spawn(effect, &args_at(v(x as f32, 0.0, 0.0)));
            fx.advance(20_000_000, &bsp);
        }
        fx.spawn(effect, &args_at(v(9.0, 0.0, 0.0)));
        let mut xs = Vec::new();
        fx.for_each(|p| xs.push(p.pos.x as i32));
        xs.sort_unstable();
        assert_eq!(fx.live(), 4);
        assert_eq!(xs, if replaced { [1, 2, 3, 9] } else { [0, 1, 2, 3] });
    }
}

#[test]
fn hostile_arguments_never_panic_or_go_non_finite() {
    let bsp = room_bsp();
    static RAMPLESS: ParticleClass = ParticleClass {
        ramp: &[],
        gravity: f32::NAN,
        drag: f32::INFINITY,
        trail: Some(Trail {
            class: ClassId(77),
            interval: f32::NAN,
        }),
        ..class(Collide::Bounce { keep: f32::NAN }, 100, Tier::Normal)
    };
    static TABLE: [ParticleClass; 3] = [
        RAMPLESS,
        class(Collide::Die, 0, Tier::Essential),
        ParticleClass {
            trail: Some(Trail {
                class: ClassId(0),
                interval: 0.0,
            }),
            gravity: 1.0e30,
            ..class(Collide::None, 100, Tier::Normal)
        },
    ];
    let bad = f32::NAN;
    static HOSTILE: [Emitter; 8] = [
        emitter(0, 20, Shape::Sphere(f32::NAN), f32::NAN, f32::INFINITY),
        emitter(1, 20, Shape::Point, 1.0, 1.0),
        emitter(200, 20, Shape::Point, 1.0, 1.0),
        emitter(
            2,
            20,
            Shape::Cone {
                half_angle: f32::INFINITY,
            },
            1.0e30,
            0.0,
        ),
        emitter(0, u16::MAX, Shape::Hemisphere, -5.0, -5.0),
        Emitter {
            count: (9, 2),
            speed: (50.0, -50.0),
            life: (2.0, 0.1),
            delay: f32::NAN,
            ..emitter(0, 0, Shape::Segment, 0.0, 0.0)
        },
        Emitter {
            delay: f32::INFINITY,
            ..emitter(2, 5, Shape::Volume, 1.0, 1.0)
        },
        Emitter {
            delay: -3.0,
            ..emitter(2, 5, Shape::Sphere(-1.0), 1.0e38, 1.0e38)
        },
    ];
    let mut fx = Fx::new(&TABLE, 3);
    let nan3 = v(bad, 0.0, 0.0);
    let nan_box = Aabb::new(nan3, v(1.0, 1.0, 1.0));
    let cases = [
        args_at(nan3),
        EffectArgs {
            dir: v(f32::INFINITY, 0.0, 0.0),
            ..args_at(Vec3::ZERO)
        },
        EffectArgs {
            end: nan3,
            ..args_at(Vec3::ZERO)
        },
        EffectArgs {
            scale: bad,
            ..args_at(Vec3::ZERO)
        },
        EffectArgs {
            scale: 1.0e30,
            ..args_at(Vec3::ZERO)
        },
        EffectArgs {
            scale: -4.0,
            ..args_at(Vec3::ZERO)
        },
        EffectArgs {
            volume: Some(nan_box),
            ..args_at(Vec3::ZERO)
        },
        args_at(v(1.0e30, 0.0, 0.0)),
        EffectArgs {
            end: v(1.0e5, -1.0e5, 3.0),
            ..args_at(Vec3::ZERO)
        },
    ];
    for a in &cases {
        fx.spawn(&HOSTILE, a);
        for _ in 0..30 {
            fx.advance(1_000_000_000, &bsp);
            assert_finite(&fx);
        }
    }
    fx.advance(u64::MAX, &bsp);
    assert_finite(&fx);
}

#[test]
fn oversized_tables_are_truncated_not_trusted() {
    static HUGE: [ParticleClass; 3] = [
        class(Collide::None, u16::MAX, Tier::Normal),
        class(Collide::None, u16::MAX, Tier::Normal),
        class(Collide::None, u16::MAX, Tier::Normal),
    ];
    let mut fx = Fx::new(&HUGE, 1);
    static E: [Emitter; 3] = [
        emitter(0, 1024, Shape::Point, 1.0, 9.0),
        emitter(1, 1024, Shape::Point, 1.0, 9.0),
        emitter(2, 1024, Shape::Point, 1.0, 9.0),
    ];
    for _ in 0..10 {
        fx.spawn(&E, &args_at(Vec3::ZERO));
    }
    assert_eq!(fx.live(), MAX_TOTAL_CAPACITY);
}

#[test]
fn a_long_stall_runs_at_most_the_catch_up_cap() {
    let bsp = room_bsp();
    static TABLE: [ParticleClass; 1] = [class(Collide::None, 4, Tier::Normal)];
    let mut fx = Fx::new(&TABLE, 1);
    static E: [Emitter; 1] = [emitter(0, 1, Shape::Point, 60.0, 50.0)];
    let mut a = args_at(Vec3::ZERO);
    a.dir = v(1.0, 0.0, 0.0);
    fx.spawn(&E, &a);
    fx.advance(10_000_000_000, &bsp);
    let mut x = 0.0;
    fx.for_each(|p| x = p.pos.x);
    // 60 units/s for four 1/60 s ticks.
    assert!((x - MAX_CATCHUP_TICKS as f32).abs() < 1.0e-3, "{x}");
}

#[test]
fn alpha_is_the_fraction_into_the_next_tick() {
    let bsp = room_bsp();
    let mut fx = Fx::new(&MIXED, 1);
    fx.advance(8_333_333, &bsp);
    assert!((fx.alpha() - 0.5).abs() < 1.0e-3);
    fx.advance(8_333_334, &bsp);
    assert!(fx.alpha() < 1.0e-3);
}

#[test]
fn gravity_and_drag_follow_semi_implicit_euler() {
    let bsp = room_bsp();
    static TABLE: [ParticleClass; 2] = [
        ParticleClass {
            gravity: 30.0,
            ..class(Collide::None, 4, Tier::Normal)
        },
        ParticleClass {
            drag: 60.0,
            ..class(Collide::None, 4, Tier::Normal)
        },
    ];
    let mut fx = Fx::new(&TABLE, 1);
    static E: [Emitter; 2] = [
        emitter(0, 1, Shape::Point, 0.0, 50.0),
        emitter(1, 1, Shape::Point, 60.0, 50.0),
    ];
    let mut a = args_at(Vec3::ZERO);
    a.dir = v(1.0, 0.0, 0.0);
    fx.spawn(&E, &a);
    fx.advance(50_000_000, &bsp);
    fx.for_each(|p| match p.class.0 {
        // Three ticks of -30/60 per tick.
        0 => assert!((p.vel.z + 1.5).abs() < 1.0e-4, "{p:?}"),
        // drag 60/s over 1/60 s removes everything in one tick.
        _ => assert_eq!(p.vel, Vec3::ZERO),
    });
}

#[test]
fn trails_shed_particles_of_their_class() {
    let bsp = room_bsp();
    static TABLE: [ParticleClass; 2] = [
        ParticleClass {
            trail: Some(Trail {
                class: ClassId(1),
                interval: 0.1,
            }),
            ..class(Collide::None, 4, Tier::Normal)
        },
        class(Collide::None, 100, Tier::Cosmetic),
    ];
    let mut fx = Fx::new(&TABLE, 1);
    static E: [Emitter; 1] = [emitter(0, 1, Shape::Point, 1.0, 5.0)];
    fx.spawn(&E, &args_at(Vec3::ZERO));
    fx.advance(1_000_000_000, &bsp);
    fx.advance(1_000_000_000, &bsp);
    // 8 ticks at 0.1 s per trail particle: 0.133 s each tick.
    let mut shed = 0;
    fx.for_each(|p| shed += usize::from(p.class.0 == 1));
    assert!((1..=2).contains(&shed), "{shed}");
}

#[test]
fn a_game_is_unchanged_by_an_fx_running_beside_it() {
    let level = open_level(vec![
        start(v(0.0, 0.0, 0.0), 0.0),
        crate::test_util::enemy(v(6.0, 0.0, 0.0)),
        far_goal(),
    ]);
    let trace = random_trace(5, 300);
    let (mut plain, mut beside) = (game(&level), game(&level));
    let bsp = game(&level).arena.bsp;
    let mut fx = Fx::new(&MIXED, 9);
    for input in &trace {
        plain.step(input);
        beside.step(input);
        fx.spawn(&BLAST, &args_at(beside.player.pos()));
        fx.advance(8_333_333, &bsp);
    }
    assert!(fx.live() > 0);
    assert_eq!(plain.player.pos(), beside.player.pos());
    assert_eq!(plain.phase, beside.phase);
    assert_eq!(plain.enemies.len(), beside.enemies.len());
    for (a, b) in plain.enemies.iter().zip(&beside.enemies) {
        assert_eq!(a.pos(), b.pos());
    }
}
