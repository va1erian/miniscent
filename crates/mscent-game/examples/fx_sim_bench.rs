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
    Blend, ClassId, Collide, EffectArgs, Emitter, Fx, FxWorld, ParticleClass, Prim, Shape, Tier,
    TintSource,
};
use mscent_math::Vec3;
use mscent_world::{Entity, showcase_level};

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

fn run(
    name: &str,
    table: &'static [ParticleClass],
    effect: &'static [Emitter],
    arena: &Arena,
    world: FxWorld,
) {
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
        fx.advance(ONE_TICK_NS, world);
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

fn mixes(arena: &Arena, world: FxWorld) {
    run("none, 4096", &NONE_4096, &NONE_ONLY, arena, world);
    run("none only", &TABLE, &NONE_ONLY, arena, world);
    run("die only", &TABLE, &DIE_ONLY, arena, world);
    run("bounce only", &TABLE, &BOUNCE_ONLY, arena, world);
    run("mixed", &TABLE, &ALL, arena, world);
}

/// The voxel cave, from the level the editor ships.
fn cave() -> Arena {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../assets/levels/voxel-cave.msl"
    );
    let bytes = std::fs::read(path).expect("the cave level is in assets/levels");
    let mut level = mscent_world::format::read(&bytes).expect("the cave level loads");
    // The level has no start: take the first spot of a coarse grid that is
    // open air, so the bursts begin in the cave.
    level.entities.push(Entity::Goal {
        pos: Vec3::ZERO,
        radius: 1.0,
    });
    for i in 0..4096_u32 {
        let at = |n: u32| (n as f32 - 8.0) * 4.0;
        let pos = Vec3::new(at(i % 16), at(i / 16 % 16), at(i / 256));
        level.entities.push(Entity::PlayerStart { pos, yaw: 0.0 });
        if let Ok(arena) = Arena::from_level(&level) {
            return arena;
        }
        level.entities.pop();
    }
    panic!("no open air in the cave level");
}

fn main() {
    let arena = Arena::from_level(&showcase_level()).expect("showcase loads");
    println!("showcase (no volumes)");
    mixes(&arena, FxWorld::from(&arena.bsp));
    let cave = cave();
    println!("voxel cave, BSP only (as before this change)");
    mixes(&cave, FxWorld::from(&cave.bsp));
    println!("voxel cave, BSP and voxels");
    mixes(&cave, cave.fx_world());
}
