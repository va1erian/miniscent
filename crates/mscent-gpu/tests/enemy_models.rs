//! Enemies drawn as their kinds' models on the GPU (M19.3): the same scene as
//! software's `enemy_models` test. The pictures need not match software's, but
//! the enemy has to be there, where it stands, and each kind its own design.

#[path = "support/gpu.rs"]
mod gpu;

use std::path::Path;

use mscent_gpu::GpuRenderer;
use mscent_math::Vec3;
use mscent_render::{
    BuildConfig, Effects, FrameInput, Framebuffer, LevelRenderer, MaterialTable, Quality, View,
};
use mscent_world::{Entity, Level, LevelBuilder};

const W: u32 = 160;
const H: u32 = 90;

/// A lit room with one stock enemy of `kind` ahead of the eye.
fn level(kind: u8) -> LevelRenderer {
    let mut b = LevelBuilder::new();
    b.room([0.0, 0.0, 0.0], [14.0, 10.0, 6.0], (0, 1, 2));
    let level = Level {
        brushes: b.finish(),
        entities: vec![
            Entity::Light {
                pos: Vec3::new(6.0, 5.0, 5.0),
                color: Vec3::new(1.0, 1.0, 1.0),
                radius: 14.0,
                intensity: 1.0,
            },
            Entity::Enemy {
                pos: Vec3::new(7.0, 5.0, 3.0),
                kind,
            },
        ],
        ..Default::default()
    };
    let cfg = BuildConfig {
        luxel_size: 4.0,
        shadow_samples: 1,
        ..BuildConfig::default()
    };
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/textures");
    let materials = MaterialTable::load_dir(&dir).expect("textures");
    LevelRenderer::from_level(&level, materials, &cfg).expect("builds")
}

/// The GPU's frame from the room's west end, looking east at the enemy.
fn frame(gpu: &mscent_gpu::Gpu, r: &LevelRenderer, models: bool) -> Framebuffer {
    let mut renderer = GpuRenderer::new(gpu.clone(), r.assets()).expect("fits");
    let view = View::from_angles(
        Vec3::new(1.0, 5.0, 3.0),
        -core::f32::consts::FRAC_PI_2,
        0.0,
        0.0,
    );
    let mut fb = Framebuffer::new(W, H).expect("a frame");
    let input = FrameInput {
        view: &view,
        quality: Quality::Low,
        actors: r.actors(),
        effects: Effects {
            models: if models {
                r.assets().spawn_models()
            } else {
                None
            },
            ..Effects::default()
        },
        wants_depth: false,
    };
    renderer.draw(r.assets(), &mut fb, &input).expect("drawn");
    fb
}

fn changed(a: &Framebuffer, b: &Framebuffer) -> Vec<(u32, u32)> {
    let pairs = a.pixels().iter().zip(b.pixels()).enumerate();
    let differ = pairs.filter(|(_, (p, q))| p != q);
    differ.map(|(i, _)| (i as u32 % W, i as u32 / W)).collect()
}

#[test]
fn an_enemy_is_its_kinds_model_where_it_stands() {
    let Some(gpu) = gpu::gpu() else { return };
    let r = level(0);
    let changed = changed(&frame(&gpu, &r, false), &frame(&gpu, &r, true));
    assert!(changed.len() > 150, "{} pixels", changed.len());
    let (sx, sy) = changed
        .iter()
        .fold((0, 0), |(x, y), &(a, b)| (x + a, y + b));
    let n = changed.len() as u32;
    let (cx, cy) = (sx / n, sy / n);
    assert!(
        cx.abs_diff(W / 2) < 12 && cy.abs_diff(H / 2) < 12,
        "at ({cx}, {cy})"
    );
}

#[test]
fn each_kind_draws_its_own_design() {
    let Some(gpu) = gpu::gpu() else { return };
    let sentry = frame(&gpu, &level(0), true);
    let dart = frame(&gpu, &level(1), true);
    assert!(changed(&sentry, &dart).len() > 100);
}
