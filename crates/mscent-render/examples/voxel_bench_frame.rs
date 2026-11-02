//! What voxel surfaces add to a frame: the 8x8x4 gyroid cave from `docs/voxels.md`
//! from 40 air positions, on one worker and at 640x360, against a brush room whose
//! pixels are all lit faces.
//!
//! `cargo run --release -p mscent-render --example voxel_bench_frame`

use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

use mscent_math::rng::Rng;
use mscent_math::{Vec3, Vec3d};
use mscent_render::{
    BuildConfig, Compiled, Framebuffer, LevelRenderer, MaterialTable, Quality, View,
};
use mscent_world::brush::{Brush, Op};
use mscent_world::materials::ROCK;
use mscent_world::voxel::{CellSize, Context, Volume, fill};
use mscent_world::{Entity, Level};

const SIZE: (u32, u32) = (640, 360);
const ROUNDS: u32 = 5;

fn gyroid(p: Vec3d) -> f64 {
    let g = |x: f64, y: f64, z: f64| x.sin() * y.cos() + y.sin() * z.cos() + z.sin() * x.cos();
    g(p.x * 0.55, p.y * 0.55, p.z * 0.55)
        + g(p.x * 1.7 + 3.0, p.y * 1.7, p.z * 1.7 + 1.0) * 0.18
        + g(p.x * 4.1, p.y * 4.3 + 2.0, p.z * 3.9) * 0.05
        + 0.25
}

fn renderer(level: &Level) -> LevelRenderer {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/textures");
    let table = MaterialTable::load_dir(&dir).expect("textures");
    let cfg = BuildConfig {
        luxel_size: 0.5,
        ..BuildConfig::default()
    };
    let c = Compiled::new(level, &cfg).expect("compiles");
    let map = Arc::new(c.draft().expect("drafts"));
    let mut r = c.into_renderer(map, table).expect("assembles");
    r.set_tiling(24, 1);
    r
}

/// Mean and worst frame over `poses`, with triangles and pixels per frame.
fn measure(r: &LevelRenderer, poses: &[(Vec3, f32)], quality: Quality) -> (f64, f64, u32, u32) {
    let mut fb = Framebuffer::new(SIZE.0, SIZE.1).expect("fb");
    let (mut sum, mut worst) = (0.0f64, 0.0f64);
    let (mut tris, mut px) = (0u32, 0u32);
    for &(eye, yaw) in poses {
        let view = View::from_angles(eye, yaw, 0.0, 0.0);
        let mut best = f64::MAX;
        for _ in 0..ROUNDS {
            let t = Instant::now();
            let stats = r
                .render(&mut fb, &view, quality, &[], None)
                .expect("renders");
            best = best.min(t.elapsed().as_secs_f64() * 1e3);
            tris = tris.max(stats.voxel_triangles);
            px = stats.pixels;
        }
        sum += best;
        worst = worst.max(best);
    }
    (sum / poses.len() as f64, worst, tris, px)
}

fn lamp(pos: Vec3) -> Entity {
    Entity::Light {
        pos,
        color: Vec3::new(1.0, 1.0, 1.0),
        radius: 40.0,
        intensity: 1.0,
    }
}

fn main() {
    let mut volume =
        Volume::new(Op::Subtract, 0, CellSize::Quarter, [0; 3], [8, 8, 4]).expect("valid");
    let shape = volume.clone();
    fill(&mut volume, &mut Context::new(&shape, &[]), gyroid, 4);
    let cave = Level {
        volumes: vec![volume],
        entities: vec![lamp(Vec3::new(16.0, 16.0, 8.0))],
        ..Default::default()
    };
    let r = renderer(&cave);
    let mut rng = Rng::new(8);
    let mut poses = Vec::new();
    while poses.len() < 40 {
        let p = Vec3d::new(
            rng.range_f64(2.0, 30.0),
            rng.range_f64(2.0, 30.0),
            rng.range_f64(2.0, 14.0),
        );
        if gyroid(p) < -0.3 {
            poses.push((p.to_f32(), rng.range_f64(-3.1, 3.1) as f32));
        }
    }
    for (name, q) in [
        ("low", Quality::Low),
        ("medium", Quality::Medium),
        ("high", Quality::High),
    ] {
        let (mean, worst, tris, _) = measure(&r, &poses, q);
        println!(
            "cave {name}: {mean:.2} ms mean, {worst:.2} ms worst, up to {tris} triangles, {:.1} ns a pixel",
            mean * 1e6 / f64::from(SIZE.0 * SIZE.1),
        );
    }
    let room = Brush::cuboid(
        Vec3d::new(-16.0, -16.0, 0.0),
        Vec3d::new(16.0, 16.0, 12.0),
        Op::Subtract,
        0,
    )
    .expect("valid")
    .with_material(ROCK);
    let brushes = Level {
        brushes: vec![room],
        entities: vec![lamp(Vec3::new(0.0, 0.0, 8.0))],
        ..Default::default()
    };
    let r = renderer(&brushes);
    let poses = [
        (Vec3::new(0.0, 0.0, 6.0), 0.5),
        (Vec3::new(4.0, 4.0, 3.0), 2.0),
    ];
    for (name, q) in [
        ("low", Quality::Low),
        ("medium", Quality::Medium),
        ("high", Quality::High),
    ] {
        let (mean, _, _, _) = measure(&r, &poses, q);
        println!(
            "brush room {name}: {mean:.2} ms mean, {:.1} ns a pixel",
            mean * 1e6 / f64::from(SIZE.0 * SIZE.1),
        );
    }
}
