//! Cost of one particle tick on the showcase level's real BSP.
//!
//! Run: `cargo run --release -p mscent-game --example fx_sim_bench`
//!
//! Each round tops the pools up (untimed) and then times a single `tick`
//! worth of `advance`. Prints the mean and worst of the timed ticks for four
//! mixes, plus the collision work a tick actually spent.

use std::time::Instant;

use mscent_game::Arena;
use mscent_game::fx::{
    Blend, ClassId, Collide, EffectArgs, Emitter, Fx, ParticleClass, Prim, Shape, Tier, TintSource,
};
use mscent_world::showcase_level;

const ROUNDS: usize = 3000;
/// 60 Hz in whole nanoseconds is not exact; this gives one tick per call.
const ONE_TICK_NS: u64 = 16_666_667;

const fn class(collide: Collide, capacity: u16) -> ParticleClass {
    ParticleClass {
        prim: Prim::Spark,
        blend: Blend::Add,
        tier: Tier::Normal,
        gravity: 3.0,
        drag: 0.3,
        collide,
        ramp: &[0xFFFF_FFFF],
        size: (0.1, 0.0),
        stretch: 0.0,
        capacity,
        trail: None,
    }
}

const fn burst(class: u8, speed: f32) -> Emitter {
    Emitter {
        class: ClassId(class),
        count: (1024, 1024),
        shape: Shape::Sphere(2.0),
        speed: (speed * 0.5, speed),
        life: (20.0, 30.0),
        delay: 0.0,
        tint: TintSource::Class,
    }
}

static NONE_4096: [ParticleClass; 1] = [class(Collide::None, 4096)];
static TABLE: [ParticleClass; 3] = [
    class(Collide::None, 1366),
    class(Collide::Die, 1365),
    class(Collide::Bounce { keep: 0.8 }, 1365),
];

/// Pool contents to hold, as emitters per class (extra particles are dropped
/// by full pools, which is what tops them up).
static ALL: [Emitter; 3] = [burst(0, 6.0), burst(1, 6.0), burst(2, 6.0)];
static NONE_ONLY: [Emitter; 1] = [burst(0, 6.0)];
static DIE_ONLY: [Emitter; 1] = [burst(1, 6.0)];
static BOUNCE_ONLY: [Emitter; 1] = [burst(2, 6.0)];

fn run(name: &str, table: &'static [ParticleClass], effect: &'static [Emitter], arena: &Arena) {
    let mut fx = Fx::new(table, 1);
    let args = EffectArgs {
        pos: arena.player_start,
        dir: mscent_math::Vec3::ZERO,
        end: arena.player_start,
        scale: 1.0,
        surface: 0,
        volume: None,
    };
    let (mut total, mut worst, mut live, mut tests, mut sweeps) = (0u128, 0u128, 0, 0, 0);
    for _ in 0..ROUNDS {
        fx.spawn(effect, &args);
        let start = Instant::now();
        fx.advance(ONE_TICK_NS, &arena.bsp);
        let ns = start.elapsed().as_nanos();
        let b = fx.last_tick_collision();
        total += ns;
        worst = worst.max(ns);
        live += fx.live();
        tests += u64::from(b.solid_tests);
        sweeps += u64::from(b.sweeps);
    }
    let n = ROUNDS as u128;
    println!(
        "{name:<22} live {:>5}  mean {:>7.1} us  worst {:>7.1} us  is_solid {:>5.0}  bounces {:>4.1} per tick",
        live / ROUNDS,
        total as f64 / n as f64 / 1000.0,
        worst as f64 / 1000.0,
        tests as f64 / ROUNDS as f64,
        sweeps as f64 / ROUNDS as f64,
    );
}

fn main() {
    let arena = Arena::from_level(&showcase_level()).expect("showcase loads");
    run("none, 4096", &NONE_4096, &NONE_ONLY, &arena);
    run("none only", &TABLE, &NONE_ONLY, &arena);
    run("die only", &TABLE, &DIE_ONLY, &arena);
    run("bounce only", &TABLE, &BOUNCE_ONLY, &arena);
    run("mixed", &TABLE, &ALL, &arena);
}
