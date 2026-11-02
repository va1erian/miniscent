//! Cost of dynamic lights on a face that fills the screen (M9.5).
//!
//! A room, with the camera three units from a wall and up to four lights
//! between them, so every pixel of the frame is a lit wall pixel. The frame
//! is timed with no lights and with some, one thread, and the difference is
//! divided by the spans the frame drew.
//!
//!   cargo run --release -p mscent-render --example light_bench -- [--lights N]
//!       [--percent N] [--frames N] [--batches N]

use std::hint::black_box;
use std::path::Path;
use std::time::Instant;

use mscent_math::{Vec3, Vec3d};
use mscent_render::{
    BuildConfig, DynLight, Effects, Framebuffer, LevelRenderer, MaterialTable, Quality, View,
};
use mscent_world::{Brush, Entity, Level, Op};

const W: u32 = 640;
const H: u32 = 360;

fn room() -> Result<LevelRenderer, String> {
    let d = |x, y, z| Vec3d::new(x, y, z);
    let brush = Brush::cuboid(d(-20.0, -20.0, -20.0), d(20.0, 20.0, 20.0), Op::Subtract, 0)
        .map_err(|e| format!("room brush: {e:?}"))?;
    let level = Level {
        brushes: vec![brush],
        entities: vec![Entity::Light {
            pos: Vec3::new(-5.0, 0.0, 5.0),
            color: Vec3::new(1.0, 1.0, 1.0),
            radius: 40.0,
            intensity: 1.0,
        }],
        labels: Vec::new(),
        mover_groups: Vec::new(),
        volumes: Vec::new(),
    };
    let cfg = BuildConfig {
        luxel_size: 0.5,
        shadow_samples: 1,
        ..BuildConfig::default()
    };
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/textures");
    let materials = MaterialTable::load_dir(&dir).map_err(|e| format!("textures: {e:?}"))?;
    LevelRenderer::from_level(&level, materials, &cfg).map_err(|e| format!("level: {e:?}"))
}

fn arg(name: &str) -> Option<usize> {
    let mut args = std::env::args();
    while let Some(a) = args.next() {
        if a == name {
            return args.next().and_then(|v| v.parse().ok());
        }
    }
    None
}

/// `count` white lights in front of the wall at x = 20, which the camera at
/// x = 17 sees edge to edge, each reaching the whole of it.
fn lights_of(r: &LevelRenderer, count: usize, gain: f32) -> Result<Vec<DynLight>, String> {
    (0..count)
        .map(|i| {
            let mut light = DynLight::new();
            let pos = Vec3::new(18.0, i as f32 - 1.5, 0.0);
            let color = Vec3::new(1.0, 0.8, 0.5) * gain;
            let placed = light.place(r.scene(), pos, 12.0, color);
            placed.then_some(light).ok_or_else(|| "light".to_string())
        })
        .collect()
}

/// Best-of-`batches` milliseconds per frame, and the spans and a checksum of
/// the last one.
fn time_frame(r: &LevelRenderer, view: &View, lights: &[DynLight]) -> (f64, u32, u32) {
    let (frames, batches) = (arg("--frames").unwrap_or(60), arg("--batches").unwrap_or(9));
    let mut fb = Framebuffer::new(W, H).expect("a target");
    let effects = Effects {
        particles: None,
        lights,
        poses: &[],
        ..Effects::default()
    };
    let (mut spans, mut best) = (0, f64::MAX);
    for _ in 0..batches {
        let t = Instant::now();
        for _ in 0..frames {
            let stats = r.render_with_effects(&mut fb, view, Quality::High, &[], None, &effects);
            spans = stats.map_or(0, |s| s.spans);
            black_box(&fb);
        }
        best = best.min(t.elapsed().as_secs_f64() * 1000.0 / frames as f64);
    }
    let sum = fb
        .pixels()
        .iter()
        .fold(0u32, |s, &p| s.wrapping_mul(31).wrapping_add(p));
    (best, spans, sum)
}

fn main() -> Result<(), String> {
    let mut r = room()?;
    r.set_tiling(24, 1);
    let view = View::from_angles(
        Vec3::new(17.0, 0.0, 0.0),
        -core::f32::consts::FRAC_PI_2,
        0.0,
        0.0,
    );
    // Per cent of full strength: a dim light keeps pixels under the knee of the
    // soft limit, which is costlier per pixel than the light itself.
    let gain = arg("--percent").unwrap_or(100) as f32 / 100.0;
    let counts = arg("--lights").map_or(vec![0, 1, 2, 4], |n| vec![n]);
    let mut base = None;
    for n in counts {
        let (ms, spans, sum) = time_frame(&r, &view, &lights_of(&r, n, gain)?);
        let base_ms = *base.get_or_insert(ms);
        let per_span = (ms - base_ms) * 1.0e6 / f64::from(spans.max(1));
        println!(
            "lights {n}: {ms:.3} ms per frame, {spans} spans, {per_span:.1} ns per span over none, checksum {sum:08x}"
        );
    }
    Ok(())
}
