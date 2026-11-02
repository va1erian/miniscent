//! What a vista costs a frame: the Monument demo at 640x360, one thread, High
//! quality, from two views. Variants take turns batch by batch, and the fastest
//! batch of each is reported, as `mover_bench` does.
//!
//!   cargo run --release -p mscent-render --example vista_bench
//!
//! - `wall`: the sky faces are plain metal, and nothing else differs.
//! - `holes`: viewport faces with no layers, so a frame marks and resolves its
//!   holes and draws the gradient: the fixed cost of the pass.
//! - `vista`: the level as shipped.

use std::path::Path;
use std::time::Instant;

use mscent_math::Vec3;
use mscent_render::{BuildConfig, Framebuffer, LevelRenderer, MaterialTable, Quality, View};
use mscent_world::materials::{METAL, view_slot};
use mscent_world::{Entity, Level, format};

const W: u32 = 640;
const H: u32 = 360;
const FRAMES: usize = 40;
const BATCHES: usize = 9;

fn demo() -> Level {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/levels/facing.msl");
    let bytes = std::fs::read(path).expect("the demo level");
    format::read(&bytes).expect("a level")
}

fn without_layers(level: &Level) -> Level {
    let mut out = level.clone();
    out.entities
        .retain(|e| !matches!(e, Entity::VistaLayer { .. }));
    out
}

/// `level` with no layers and metal for every viewport face.
fn walled(level: &Level) -> Level {
    let mut out = without_layers(level);
    for brush in &mut out.brushes {
        if view_slot(brush.material()).is_some() {
            *brush = brush.clone().with_material(METAL);
        }
    }
    out
}

fn frame_batch(r: &LevelRenderer, fb: &mut Framebuffer, view: &View) -> f64 {
    let t = Instant::now();
    for _ in 0..FRAMES {
        r.render(fb, view, Quality::High, &[], None)
            .expect("renders");
    }
    t.elapsed().as_secs_f64() * 1e3 / FRAMES as f64
}

fn main() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/textures");
    let materials = MaterialTable::load_dir(&dir).expect("textures");
    let level = demo();
    let cfg = BuildConfig::default();
    let variants = [
        ("wall", walled(&level)),
        ("holes", without_layers(&level)),
        ("vista", level.clone()),
    ];
    let mut renderers: Vec<(&str, LevelRenderer)> = variants
        .iter()
        .map(|(name, l)| {
            let r = LevelRenderer::from_level(l, materials.clone(), &cfg).expect("builds");
            (*name, r)
        })
        .collect();
    for (_, r) in &mut renderers {
        r.set_tiling(H, 1);
    }
    let eye = Vec3::new(0.0, -22.0, 0.0);
    let poses = [
        ("level, at the gate", View::from_angles(eye, 0.0, 0.0, 0.0)),
        (
            "looking up, sky only",
            View::from_angles(eye, 0.0, 0.9, 0.0),
        ),
    ];
    let mut fb = Framebuffer::new(W, H).expect("fb");
    for (label, view) in &poses {
        let mut best = [f64::MAX; 3];
        for _ in 0..BATCHES {
            for (slot, (_, r)) in best.iter_mut().zip(&renderers) {
                *slot = slot.min(frame_batch(r, &mut fb, view));
            }
        }
        println!("{label}:");
        for ((name, r), ms) in renderers.iter().zip(best) {
            let stats = r
                .render(&mut fb, view, Quality::High, &[], None)
                .expect("renders");
            println!(
                "  {name:6} {ms:6.3} ms  {:6} pixels, {:6} from the vista, {:3} vista faces",
                stats.pixels, stats.vista_pixels, stats.vista_faces
            );
        }
    }
}
