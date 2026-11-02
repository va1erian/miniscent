//! What movers cost a frame: the showcase from a fixed view, with no movers and
//! with several in view, at 640x360 on one thread, High quality.
//!
//!   cargo run --release -p mscent-render --example mover_bench
//!
//! The fastest batch of each is reported, which discards interference. Results
//! are recorded in docs/perf/movers.md.

use std::hint::black_box;
use std::path::Path;
use std::time::Instant;

use mscent_math::{Vec3, Vec3d};
use mscent_render::{BuildConfig, Framebuffer, LevelRenderer, MaterialTable, Quality, View};
use mscent_world::{Brush, Level, MoverGroup, Op, showcase_level};

const W: u32 = 640;
const H: u32 = 360;
const FRAMES: usize = 30;
const BATCHES: usize = 7;

/// The showcase with `count` box doors in the start room.
fn level(count: usize) -> Level {
    let mut level = showcase_level();
    for i in 0..count {
        let x = 2.0 + i as f64 * 0.8;
        let door = Brush::cuboid(
            Vec3d::new(x, 6.0, 1.0),
            Vec3d::new(x + 0.5, 6.5, 3.0),
            Op::Add,
            9000,
        )
        .expect("door");
        level.brushes.push(door);
        level.mover_groups.push(MoverGroup {
            name: format!("door{i}"),
            tags: Vec::new(),
            brushes: vec![level.brushes.len() as u32 - 1],
        });
    }
    level
}

fn batch_ms(r: &LevelRenderer, fb: &mut Framebuffer, view: &View) -> f64 {
    let t = Instant::now();
    for _ in 0..FRAMES {
        r.render_posed(fb, view, Quality::High, &[], None, &[])
            .expect("renders");
        black_box(fb.pixels());
    }
    t.elapsed().as_secs_f64() * 1000.0 / FRAMES as f64
}

fn main() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/textures");
    let view = View::from_angles(Vec3::new(3.0, 1.0, 2.0), 0.0, 0.0, 0.0);
    let counts = [0, 1, 4, 16];
    let renderers: Vec<LevelRenderer> = counts
        .iter()
        .map(|&count| {
            let materials = MaterialTable::load_dir(&dir).expect("textures");
            LevelRenderer::from_level(&level(count), materials, &BuildConfig::default())
                .expect("builds")
        })
        .collect();
    let mut fb = Framebuffer::new(W, H).expect("framebuffer");
    let mut best = [f64::MAX; 4];
    // Taking turns, so a machine whose clock drifts during the run treats every
    // count alike.
    for _ in 0..BATCHES {
        for (r, b) in renderers.iter().zip(&mut best) {
            *b = b.min(batch_ms(r, &mut fb, &view));
        }
    }
    for (count, ms) in counts.iter().zip(best) {
        println!("{count} movers {ms:.3} ms/frame");
    }
}
