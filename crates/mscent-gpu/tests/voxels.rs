//! Voxel surfaces on the GPU, read back as pixels: a closed cave shows no sky
//! from anywhere inside it, as `voxel/fill_tests.rs` holds software to, and a
//! sculpt between frames reaches the picture without a new renderer.

#[path = "support/compare.rs"]
mod compare;
#[path = "support/gpu.rs"]
mod gpu;

use std::path::Path;
use std::sync::Arc;

use compare::{Frame, compare};
use gpu::gpu;
use mscent_gpu::GpuRenderer;
use mscent_math::rng::Rng;
use mscent_math::{Vec3, Vec3d};
use mscent_render::{
    BuildConfig, Compiled, Effects, FrameInput, Framebuffer, LevelRenderer, MaterialTable, Quality,
    View, VoxelChange,
};
use mscent_world::voxel::{CellSize, Context, DabOp, Prim, Volume, VoxelWorld, apply_dab, fill};
use mscent_world::{Entity, Level, Op};

/// The sky, in a colour nothing else has, so a hole cannot hide.
const HOLE: u32 = 0xffff_00ff;

/// A warped gyroid, positive solid: `gyroid_level` in `fill_tests.rs`.
fn gyroid(p: Vec3d) -> f64 {
    let g = |x: f64, y: f64, z: f64| x.sin() * y.cos() + y.sin() * z.cos() + z.sin() * x.cos();
    g(p.x * 0.55, p.y * 0.55, p.z * 0.55) + g(p.x * 1.7 + 3.0, p.y * 1.7, p.z * 1.7) * 0.18 + 0.25
}

fn cave() -> Level {
    let mut volume =
        Volume::new(Op::Subtract, 0, CellSize::Quarter, [0; 3], [4, 4, 2]).expect("a volume");
    let mut ctx = Context::new(&volume.clone(), &[]);
    fill(&mut volume, &mut ctx, gyroid, 4);
    Level {
        volumes: vec![volume],
        entities: vec![Entity::Light {
            pos: Vec3::new(8.0, 8.0, 4.0),
            color: Vec3::new(1.0, 1.0, 1.0),
            radius: 30.0,
            intensity: 1.0,
        }],
        ..Level::default()
    }
}

fn software(level: &Level) -> LevelRenderer {
    let cfg = BuildConfig {
        luxel_size: 1.0,
        shadow_samples: 1,
        sky_top: HOLE,
        sky_bottom: HOLE,
        ..BuildConfig::default()
    };
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/textures");
    let materials = MaterialTable::load_dir(&dir).expect("textures");
    let compiled = Compiled::new(level, &cfg).expect("compiles");
    let map = Arc::new(compiled.draft().expect("drafts"));
    compiled.into_renderer(map, materials).expect("assembles")
}

/// Air well inside the cave, away from every surface.
fn air_pose(rng: &mut Rng) -> Vec3 {
    loop {
        let p = Vec3d::new(
            rng.range_f64(2.5, 13.5),
            rng.range_f64(2.5, 13.5),
            rng.range_f64(2.0, 6.0),
        );
        if gyroid(p) < -0.45 {
            return p.to_f32();
        }
    }
}

fn holes(fb: &Framebuffer) -> usize {
    fb.pixels().iter().filter(|&&p| p == HOLE).count()
}

fn draw(gpu: &mut GpuRenderer, level: &LevelRenderer, fb: &mut Framebuffer, view: &View) {
    let input = FrameInput {
        view,
        quality: Quality::Low,
        actors: &[],
        effects: Effects::default(),
        wants_depth: true,
    };
    let stats = gpu.draw(level.assets(), fb, &input).expect("a frame");
    assert!(stats.voxel_triangles > 0, "no voxels in view");
    assert_eq!(gpu.take_error(), None);
}

#[test]
fn no_sky_pixel_from_inside_a_closed_cave_over_a_thousand_poses() {
    let Some(device) = gpu() else { return };
    let level = software(&cave());
    let mut renderer = GpuRenderer::new(device, level.assets()).expect("fits");
    let mut rng = Rng::new(11);
    let sizes = [(160, 90), (321, 181), (640, 360)];
    for i in 0..1000 {
        let (w, h) = sizes[i % sizes.len()];
        let mut fb = Framebuffer::new(w, h).expect("a framebuffer");
        let eye = air_pose(&mut rng);
        let yaw = rng.range_f64(-3.1, 3.1) as f32;
        let pitch = rng.range_f64(-1.2, 1.2) as f32;
        let view = View::from_angles(eye, yaw, pitch, 0.0);
        draw(&mut renderer, &level, &mut fb, &view);
        let found = holes(&fb);
        assert_eq!(found, 0, "pose {i}: eye {eye:?} yaw {yaw} pitch {pitch}");
    }
}

/// Enough frames of one view for the detail governor to settle, on both.
fn settled(gpu: &mut GpuRenderer, level: &LevelRenderer, view: &View) -> (Frame, Frame) {
    let (mut hw, mut sw) = (
        Framebuffer::new(320, 180).expect("a framebuffer"),
        Framebuffer::new(320, 180).expect("a framebuffer"),
    );
    for _ in 0..80 {
        draw(gpu, level, &mut hw, view);
        level
            .render(&mut sw, view, Quality::Low, &[], None)
            .expect("a frame");
    }
    (Frame::of(&hw), Frame::of(&sw))
}

#[test]
fn a_sculpt_reaches_the_picture_and_restores_it_exactly() {
    let Some(device) = gpu() else { return };
    let level_data = cave();
    let mut level = software(&level_data);
    let mut renderer = GpuRenderer::new(device, level.assets()).expect("fits");
    let eye = air_pose(&mut Rng::new(1));
    let view = View::from_angles(eye, 0.4, 0.0, 0.0);
    let (before, _) = settled(&mut renderer, &level, &view);
    let world = level.scene().voxels().world().clone();
    let brushes = level_data.static_brushes();
    let bsp = level.scene().bsp().clone();
    let mut volumes = level_data.volumes.clone();
    let shape = volumes[0].clone();
    let bite = Prim::sphere(eye.to_f64() + Vec3d::new(0.0, 2.0, 0.0), 1.5).expect("a ball");
    let mut ctx = Context::new(&shape, &brushes);
    let chunks = apply_dab(&mut volumes[0], &mut ctx, &bite, DabOp::Carve);
    let list = VoxelWorld::remesh(&volumes, &brushes, &bsp, &[(0, chunks)]);
    level.apply_voxels(VoxelChange::Chunks(list));
    let (carved, reference) = settled(&mut renderer, &level, &view);
    assert_ne!(carved.pixels, before.pixels, "the carve does not show");
    assert_eq!(carved.pixels.iter().filter(|&&p| p == HOLE).count(), 0);
    let report = compare(&reference, &carved);
    assert!(report.alike(), "{report:?}");
    level.apply_voxels(VoxelChange::Restore(Box::new(world)));
    let (restored, _) = settled(&mut renderer, &level, &view);
    assert_eq!(restored.pixels, before.pixels, "restored bit for bit");
}
