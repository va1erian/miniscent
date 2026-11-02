//! The three collision levels and their per-tick limits, in the closed
//! `[-20, 20]^3` test room.

use super::fx_tests::{args_at, class, emitter, room_bsp};
use super::*;
use crate::test_util::v;
use mscent_world::Bsp;

fn solid(bsp: &Bsp, p: Vec3) -> bool {
    bsp.is_solid(p.to_f64())
}

/// Position of every particle of pool `ci`, without the render lead.
fn positions(fx: &Fx, ci: usize) -> Vec<Vec3> {
    (0..fx.pools[ci].len())
        .map(|i| fx.pools[ci].pos(i))
        .collect()
}

const TABLE: [ParticleClass; 3] = [
    class(Collide::None, 1000, Tier::Normal),
    class(Collide::Die, 1000, Tier::Normal),
    class(Collide::Bounce { keep: 0.5 }, 1000, Tier::Normal),
];

fn toward_wall(class: u8, speed: f32) -> (Effect, EffectArgs) {
    let e: &'static [Emitter] = Box::leak(Box::new([emitter(class, 1, Shape::Point, speed, 10.0)]));
    let mut a = args_at(v(19.5, 0.0, 0.0));
    a.dir = v(1.0, 0.0, 0.0);
    (e, a)
}

#[test]
fn none_passes_through_walls() {
    let bsp = room_bsp();
    let mut fx = Fx::new(&TABLE, 1);
    let (e, a) = toward_wall(0, 120.0);
    fx.spawn(e, &a);
    fx.tick(&bsp);
    fx.tick(&bsp);
    assert_eq!(fx.live(), 1);
    assert!(solid(&bsp, positions(&fx, 0)[0]));
    assert_eq!(fx.last_tick_collision(), Budget::default());
}

#[test]
fn die_dies_on_entry_with_one_test() {
    let bsp = room_bsp();
    let mut fx = Fx::new(&TABLE, 1);
    let (e, a) = toward_wall(1, 60.0);
    fx.spawn(e, &a);
    fx.tick(&bsp); // 19.5 -> 20.5: inside the wall
    assert_eq!(fx.live(), 0);
    assert_eq!(
        fx.last_tick_collision(),
        Budget {
            solid_tests: 1,
            sweeps: 0
        }
    );
}

#[test]
fn bounce_reflects_keeps_speed_fraction_and_stays_outside() {
    let bsp = room_bsp();
    let mut fx = Fx::new(&TABLE, 1);
    let (e, a) = toward_wall(2, 60.0);
    fx.spawn(e, &a);
    fx.tick(&bsp);
    assert_eq!(fx.live(), 1);
    let (p, vel) = (fx.pools[2].pos(0), fx.pools[2].vel(0));
    assert!(!solid(&bsp, p));
    assert!(p.x < 20.0 && p.x > 19.5, "{p:?}");
    assert!(
        (vel.x + 30.0).abs() < 1.0e-3 && vel.y.abs() < 1.0e-3,
        "{vel:?}"
    );
    assert_eq!(
        fx.last_tick_collision(),
        Budget {
            solid_tests: 1,
            sweeps: 1
        }
    );
}

static SWARM: [Emitter; 3] = [
    emitter(0, 700, Shape::Sphere(10.0), 400.0, 20.0),
    emitter(1, 700, Shape::Sphere(10.0), 400.0, 20.0),
    emitter(2, 700, Shape::Sphere(10.0), 400.0, 20.0),
];

#[test]
fn limits_are_honoured_and_reached_under_load() {
    let bsp = room_bsp();
    let mut fx = Fx::new(&TABLE, 4);
    let (mut max_tests, mut max_sweeps) = (0, 0);
    for round in 0..40 {
        if round % 4 == 0 {
            fx.spawn(&SWARM, &args_at(v(0.0, 0.0, 0.0)));
        }
        fx.tick(&bsp);
        let b = fx.last_tick_collision();
        assert!(
            b.solid_tests <= MAX_SOLID_TESTS && b.sweeps <= MAX_SWEEPS,
            "{b:?}"
        );
        (max_tests, max_sweeps) = (max_tests.max(b.solid_tests), max_sweeps.max(b.sweeps));
        for p in positions(&fx, 2) {
            assert!(!solid(&bsp, p), "bounce particle in solid at {p:?}");
        }
    }
    assert_eq!((max_tests, max_sweeps), (MAX_SOLID_TESTS, MAX_SWEEPS));
}

#[test]
fn bounce_never_ends_a_tick_in_solid_even_at_a_million_units_a_second() {
    let bsp = room_bsp();
    static TABLE: [ParticleClass; 1] = [class(Collide::Bounce { keep: 0.9 }, 400, Tier::Normal)];
    static FAST: [Emitter; 3] = [
        emitter(0, 100, Shape::Sphere(18.0), 1.0e6, 30.0),
        emitter(0, 100, Shape::Hemisphere, 60.0, 30.0),
        emitter(0, 100, Shape::Cone { half_angle: 3.0 }, 5.0e3, 30.0),
    ];
    let mut fx = Fx::new(&TABLE, 11);
    let mut a = args_at(v(2.0, -3.0, 4.0));
    a.dir = v(0.3, 0.9, -0.2);
    fx.spawn(&FAST, &a);
    for _ in 0..1200 {
        fx.tick(&bsp);
        for p in positions(&fx, 0) {
            assert!(!solid(&bsp, p), "bounce particle in solid at {p:?}");
        }
        let b = fx.last_tick_collision();
        assert!(b.solid_tests <= MAX_SOLID_TESTS && b.sweeps <= MAX_SWEEPS);
    }
    assert!(fx.live() > 0, "everything died; the test proves nothing");
}

#[test]
fn a_bounce_particle_spawned_inside_solid_dies() {
    let bsp = room_bsp();
    let mut fx = Fx::new(&TABLE, 1);
    let e: &'static [Emitter] = Box::leak(Box::new([emitter(2, 5, Shape::Point, 1.0, 5.0)]));
    fx.spawn(e, &args_at(v(30.0, 0.0, 0.0)));
    fx.tick(&bsp);
    assert_eq!(fx.live(), 0);
}

#[test]
fn bounce_particles_get_the_test_budget_before_die_particles() {
    let bsp = room_bsp();
    let mut fx = Fx::new(&TABLE, 2);
    // Die first in the table order of the swarm, yet Bounce is stepped first.
    static MANY: [Emitter; 2] = [
        emitter(1, 1000, Shape::Sphere(5.0), 1.0, 10.0),
        emitter(2, 400, Shape::Sphere(5.0), 1.0, 10.0),
    ];
    fx.spawn(&MANY, &args_at(Vec3::ZERO));
    fx.tick(&bsp);
    assert_eq!(fx.pools[2].len(), 400, "no bounce particle may be starved");
    assert_eq!(fx.last_tick_collision().solid_tests, MAX_SOLID_TESTS);
}

#[test]
fn particles_lying_on_a_floor_spend_no_queries_and_all_survive() {
    static FALLING: [ParticleClass; 1] = [ParticleClass {
        gravity: 9.0,
        ..class(Collide::Bounce { keep: 0.35 }, 200, Tier::Normal)
    }];
    static DROP: [Emitter; 1] = [emitter(0, 200, Shape::Sphere(0.5), 0.0, 100.0)];
    let bsp = room_bsp();
    let mut fx = Fx::new(&FALLING, 1);
    fx.spawn(&DROP, &args_at(v(0.0, 0.0, -19.0)));
    for _ in 0..240 {
        fx.tick(&bsp);
    }
    assert_eq!(fx.live(), 200, "more than the 32 queries a tick allows");
    assert_eq!(fx.last_tick_collision().sweeps, 0, "all at rest");
    assert!(positions(&fx, 0).iter().all(|&p| !solid(&bsp, p)));
}
