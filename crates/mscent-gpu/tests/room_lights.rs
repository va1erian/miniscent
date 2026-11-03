//! The GPU lights the rooms a light's row of the room table reaches, read from
//! its own frames: a light beside a wall leaves the room behind it dark, a
//! light in a doorway lights both rooms, and one that moves lights the room it
//! moved to. The face shader reads a face's set of rooms from its flags
//! (`face.wgsl`), so these fail if the set is wrong or ignored.
//!
//! A machine with no GPU skips this, unless `MSCENT_REQUIRE_GPU` is set.

#[path = "support/gpu.rs"]
mod gpu;

use std::path::Path;

use gpu::gpu;
use mscent_gpu::{Gpu, GpuRenderer};
use mscent_math::{Vec3, Vec3d};
use mscent_render::{
    BuildConfig, DynLight, Effects, FrameInput, Framebuffer, LevelRenderer, MaterialTable, Quality,
    View,
};
use mscent_world::{Brush, Level, Op};

fn cuboid(a: [f64; 3], b: [f64; 3], order: u32) -> Brush {
    let v = |p: [f64; 3]| Vec3d::new(p[0], p[1], p[2]);
    Brush::cuboid(v(a), v(b), Op::Subtract, order).expect("a brush")
}

struct OnGpu {
    level: LevelRenderer,
    renderer: GpuRenderer,
}

fn on_gpu(gpu: Gpu, brushes: Vec<Brush>) -> OnGpu {
    let level = Level {
        brushes,
        entities: Vec::new(),
        labels: Vec::new(),
        mover_groups: Vec::new(),
        volumes: Vec::new(),
    };
    let cfg = BuildConfig {
        luxel_size: 1.0,
        shadow_samples: 1,
        ..BuildConfig::default()
    };
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/textures");
    let materials = MaterialTable::load_dir(&dir).expect("textures");
    let level = LevelRenderer::from_level(&level, materials, &cfg).expect("a level");
    let renderer = GpuRenderer::new(gpu, level.assets()).expect("the level fits on the GPU");
    OnGpu { level, renderer }
}

const WEST: [[f64; 3]; 2] = [[-20.0, -10.0, -10.0], [0.0, 10.0, 10.0]];
const EAST: [[f64; 3]; 2] = [[4.0, -10.0, -10.0], [24.0, 10.0, 10.0]];

/// Two rooms with a wall 4 thick between them, joined only by a U of corridor
/// that goes 100 up and comes down again.
fn walled() -> Vec<Brush> {
    vec![
        cuboid(WEST[0], WEST[1], 0),
        cuboid(EAST[0], EAST[1], 1),
        cuboid([-12.0, -2.0, 9.0], [-8.0, 2.0, 100.0], 2),
        cuboid([-12.0, -2.0, 96.0], [16.0, 2.0, 100.0], 3),
        cuboid([12.0, -2.0, 9.0], [16.0, 2.0, 100.0], 4),
    ]
}

/// The same rooms with a doorway through the wall.
fn doorway() -> Vec<Brush> {
    vec![
        cuboid(WEST[0], WEST[1], 0),
        cuboid(EAST[0], EAST[1], 1),
        cuboid([0.0, -3.0, -3.0], [4.0, 3.0, 3.0], 2),
    ]
}

fn v(x: f32, y: f32, z: f32) -> Vec3 {
    Vec3::new(x, y, z)
}

fn west_view() -> View {
    View::from_angles(v(-14.0, 0.0, 6.0), -1.2, -0.6, 0.0)
}

fn east_view() -> View {
    View::from_angles(v(14.0, 0.0, 6.0), 1.2, -0.6, 0.0)
}

fn frame(on: &mut OnGpu, view: &View, lights: &[DynLight]) -> Vec<u32> {
    let mut fb = Framebuffer::new(160, 90).expect("a target");
    let input = FrameInput {
        view,
        quality: Quality::Medium,
        actors: &[],
        effects: Effects {
            lights,
            ..Effects::default()
        },
        wants_depth: false,
    };
    on.renderer
        .draw(on.level.assets(), &mut fb, &input)
        .expect("a frame");
    if let Some(error) = on.renderer.take_error() {
        panic!("the GPU reported: {error}");
    }
    fb.pixels().to_vec()
}

/// How many pixels the lights change.
fn changed(on: &mut OnGpu, view: &View, lights: &[DynLight]) -> usize {
    let (dark, lit) = (frame(on, view, &[]), frame(on, view, lights));
    dark.iter().zip(&lit).filter(|(a, b)| a != b).count()
}

fn place(on: &OnGpu, light: &mut DynLight, pos: Vec3) {
    assert!(light.place(on.level.scene(), pos, 20.0, v(2.0, 2.0, 2.0)));
}

#[test]
fn a_light_beside_a_wall_leaves_the_far_room_dark_on_the_gpu() {
    let Some(gpu) = gpu() else { return };
    let mut on = on_gpu(gpu, walled());
    let mut light = [DynLight::new()];
    place(&on, &mut light[0], v(-2.0, 0.0, -8.0));
    let west = changed(&mut on, &west_view(), &light);
    let east = changed(&mut on, &east_view(), &light);
    assert!(west > 1000 && east == 0, "west {west}, east {east}");
}

#[test]
fn a_light_in_a_doorway_lights_both_rooms_on_the_gpu() {
    let Some(gpu) = gpu() else { return };
    let mut on = on_gpu(gpu, doorway());
    let mut light = [DynLight::new()];
    place(&on, &mut light[0], v(2.0, 0.0, -2.5));
    let west = changed(&mut on, &west_view(), &light);
    let east = changed(&mut on, &east_view(), &light);
    assert!(west > 1000 && east > 1000, "west {west}, east {east}");
}

#[test]
fn a_moving_light_lights_the_room_it_is_in_on_the_gpu() {
    let Some(gpu) = gpu() else { return };
    let mut on = on_gpu(gpu, walled());
    let mut light = [DynLight::new()];
    place(&on, &mut light[0], v(-2.0, 0.0, -8.0));
    assert!(changed(&mut on, &west_view(), &light) > 1000);
    place(&on, &mut light[0], v(6.0, 0.0, -8.0));
    let west = changed(&mut on, &west_view(), &light);
    let east = changed(&mut on, &east_view(), &light);
    assert!(west == 0 && east > 1000, "west {west}, east {east}");
}
