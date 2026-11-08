//! A GPU too small for the shadow maps still draws everything but the shadows: the
//! frame with shadows on is the frame with them off, nothing is reported, and the
//! renderer lives. Each GPU here is the real one with a limit turned down: maps
//! 768 a side, 24 layers (a cube a face of 2 lights and 2 lamps), and a layout that holds
//! them as a cube array, which wants 6 layers.
//!
//! A machine with no GPU skips this, unless `MSCENT_REQUIRE_GPU` is set.

#[path = "support/gpu.rs"]
mod gpu_support;
#[allow(dead_code)]
#[path = "support/shade.rs"]
mod shade;

use mscent_gpu::Gpu;
use mscent_math::Vec3;
use shade::{Contents, OFF, ON, Room, Shot, box_at, lamp, room, view};

/// Draws a lit room with a box in the light's way, shadows on and off, on `gpu`.
fn frames(gpu: &Gpu) -> (Vec<u32>, Vec<u32>) {
    let mut shot = Shot::new(gpu, room(Room::default()));
    let lights = [lamp(&shot.level, Vec3::new(0.0, 0.0, 3.0))];
    let boxes = [box_at(Vec3::new(0.0, 0.0, -1.0))];
    let contents = Contents {
        lights: &lights,
        boxes: &boxes,
        ..Contents::default()
    };
    let on = shot.frame(&view(), ON, &contents);
    // Twice, as a second frame is where a half-made pass would show.
    assert_eq!(shot.frame(&view(), ON, &contents), on, "steady");
    (on, shot.frame(&view(), OFF, &contents))
}

fn small(cap: impl FnOnce(wgpu::Limits) -> wgpu::Limits) -> Option<Gpu> {
    gpu_support::gpu()?;
    Some(Gpu::with_limits(cap).expect("lower limits are always allowed"))
}

fn assert_without_shadows(gpu: &Gpu, what: &str) {
    let (on, off) = frames(gpu);
    assert_eq!(on, off, "{what}: shadows on draw the frame without them");
    assert_eq!(gpu.take_error(), None, "{what}: nothing is reported");
}

#[test]
fn a_gpu_with_small_textures_draws_the_frame_without_shadows() {
    // The room's materials are 256 a side, and the maps 768.
    let Some(gpu) = small(|o| wgpu::Limits {
        max_texture_dimension_2d: 512,
        ..o
    }) else {
        return;
    };
    assert_without_shadows(&gpu, "512 a side");
}

#[test]
fn a_gpu_with_too_few_layers_for_the_maps_draws_the_frame_without_shadows() {
    // Enough for one cube, as the faces' layout wants, and not for twelve.
    let Some(gpu) = small(|o| wgpu::Limits {
        max_texture_array_layers: 8,
        ..o
    }) else {
        return;
    };
    assert_without_shadows(&gpu, "8 layers");
}

#[test]
fn a_gpu_with_too_few_layers_for_a_cube_array_builds_the_faces_without_the_lookup() {
    let Some(gpu) = small(|o| wgpu::Limits {
        max_texture_array_layers: 4,
        ..o
    }) else {
        return;
    };
    assert_without_shadows(&gpu, "4 layers");
}

/// The control: the real GPU does draw them, so the equalities above mean something.
#[test]
fn the_same_scene_on_the_real_gpu_is_shaded() {
    let Some(gpu) = gpu_support::gpu() else {
        return;
    };
    let (on, off) = frames(&gpu);
    assert_ne!(on, off, "the box casts a shadow");
}
