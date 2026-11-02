//! Level reader fuzz target. With file arguments it feeds each file to the
//! reader (the shape a cargo-fuzz harness would take); with none it mutates
//! a valid level `ITERS` times (default 200000) from a seed.
//!
//! `cargo run -p mscent-world --release --example level_fuzz -- [files...]`
//! Env: `SEED`, `ITERS`.

use mscent_math::rng::Rng;
use mscent_math::{Vec3, Vec3d};
use mscent_world::format;
use mscent_world::{Brush, Entity, Level, Op};

fn seed_level() -> Level {
    let b = |lo: f64, hi: f64, op, order| {
        Brush::cuboid(Vec3d::new(lo, lo, lo), Vec3d::new(hi, hi, hi), op, order)
    };
    Level {
        brushes: [b(-8.0, 8.0, Op::Subtract, 0), b(-1.0, 1.0, Op::Add, 1)]
            .into_iter()
            .flatten()
            .collect(),
        entities: vec![
            Entity::PlayerStart {
                pos: Vec3::new(4.0, 4.0, 4.0),
                yaw: 0.5,
            },
            Entity::Goal {
                pos: Vec3::new(-4.0, -4.0, -4.0),
                radius: 1.0,
            },
        ],
        volumes: Vec::new(),
    }
}

fn env_u64(name: &str, default: u64) -> u64 {
    std::env::var(name)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

fn mutate(rng: &mut Rng, bytes: &[u8]) -> Vec<u8> {
    let mut data = bytes.to_vec();
    for _ in 0..=rng.below(6) {
        let i = rng.below(u32::try_from(data.len()).unwrap_or(1)) as usize;
        data[i] = match rng.below(3) {
            0 => data[i] ^ (1 << rng.below(8)),
            1 => 0xff,
            _ => u8::try_from(rng.below(256)).unwrap_or(0),
        };
    }
    data
}

fn main() {
    let files: Vec<String> = std::env::args().skip(1).collect();
    if !files.is_empty() {
        for f in files {
            let ok = std::fs::read(&f).map(|d| format::read(&d).is_ok());
            println!("{f}: {ok:?}");
        }
        return;
    }
    let seed_bytes = format::write(&seed_level());
    let mut rng = Rng::new(env_u64("SEED", 1));
    let (mut accepted, iters) = (0u64, env_u64("ITERS", 200_000));
    for _ in 0..iters {
        if format::read(&mutate(&mut rng, &seed_bytes)).is_ok() {
            accepted += 1;
        }
    }
    println!("{iters} mutations, {accepted} still parsed, no panics");
}
