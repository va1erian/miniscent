//! What the GPU renderer does when the GPU fails it: a lost device, and a GPU
//! smaller than a level or a frame. Each is an error value naming what went,
//! never a panic and never a wgpu error left for nobody.

#[path = "support/gpu.rs"]
mod gpu;

use std::path::Path;

use mscent_gpu::{Gpu, GpuError, GpuRenderer};
use mscent_math::Vec3;
use mscent_render::{
    Actor, ActorKind, BuildConfig, Effects, FrameInput, Framebuffer, LevelRenderer, MaterialTable,
    Quality, View,
};
use mscent_world::{Entity, Level, LevelBuilder};

/// A 10 by 10 room, lit, with luxels large enough that its atlas is small.
fn room() -> LevelRenderer {
    let mut b = LevelBuilder::new();
    b.room([0.0, 0.0, 0.0], [10.0, 10.0, 6.0], (0, 1, 2));
    let level = Level {
        brushes: b.finish(),
        entities: vec![Entity::Light {
            pos: Vec3::new(5.0, 5.0, 4.0),
            color: Vec3::new(1.0, 1.0, 1.0),
            radius: 12.0,
            intensity: 1.0,
        }],
        ..Default::default()
    };
    let cfg = BuildConfig {
        luxel_size: 4.0,
        shadow_samples: 1,
        ..BuildConfig::default()
    };
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/textures");
    let materials = MaterialTable::load_dir(&dir).expect("textures");
    LevelRenderer::from_level(&level, materials, &cfg).expect("the room builds")
}

fn view() -> View {
    View::from_angles(Vec3::new(2.0, 2.0, 2.0), 0.7, 0.0, 0.0)
}

/// Draws `level` from `view()` at `w` by `h`, with `actors`.
fn draw(
    renderer: &mut GpuRenderer,
    level: &LevelRenderer,
    (w, h): (u32, u32),
    actors: &[Actor],
) -> bool {
    let mut fb = Framebuffer::new(w, h).expect("a framebuffer");
    let input = FrameInput {
        view: &view(),
        quality: Quality::Low,
        actors,
        effects: Effects::default(),
        wants_depth: true,
    };
    renderer.draw(level.assets(), &mut fb, &input).is_some()
}

/// A GPU with `cap` taken from what it offers, or `None` without one.
fn smaller_gpu(cap: impl FnOnce(wgpu::Limits) -> wgpu::Limits) -> Option<Gpu> {
    gpu::gpu()?;
    Some(Gpu::with_limits(cap).expect("lower limits are always allowed"))
}

#[test]
fn a_lost_device_is_an_error_and_never_a_panic() {
    let Some(device) = gpu::gpu() else { return };
    let level = room();
    let mut renderer = GpuRenderer::new(device.clone(), level.assets()).expect("fits");
    assert!(draw(&mut renderer, &level, (64, 48), &[]), "drew before");
    assert_eq!(renderer.take_error(), None);
    device.destroy();
    assert!(device.lost().is_some());
    assert!(!draw(&mut renderer, &level, (64, 48), &[]), "drew after");
    let why = renderer.take_error().expect("a reason");
    assert!(why.contains("lost"), "{why}");
    // A new size makes new targets, on a device that is not there.
    assert!(!draw(&mut renderer, &level, (32, 16), &[]));
    let again = GpuRenderer::new(device.clone(), level.assets()).err();
    assert!(matches!(again, Some(GpuError::Lost(_))), "{again:?}");
    assert!(!draw(&mut renderer, &level, (64, 48), &[]));
}

#[test]
fn a_frame_past_the_largest_texture_is_an_error_naming_it() {
    let Some(device) = smaller_gpu(|offered| wgpu::Limits {
        max_texture_dimension_2d: 512,
        ..offered
    }) else {
        return;
    };
    let level = room();
    let mut renderer = GpuRenderer::new(device, level.assets()).expect("fits");
    assert!(!draw(&mut renderer, &level, (513, 64), &[]));
    let why = renderer.take_error().expect("a reason");
    assert!(why.contains("too large for this GPU: the frame"), "{why}");
    assert!(!draw(&mut renderer, &level, (64, 600), &[]));
    assert!(renderer.take_error().is_some());
    assert!(
        draw(&mut renderer, &level, (512, 64), &[]),
        "the largest fits"
    );
    assert_eq!(renderer.take_error(), None);
}

#[test]
fn a_lightmap_past_the_largest_texture_is_an_error_naming_it() {
    let Some(device) = smaller_gpu(|offered| wgpu::Limits {
        max_texture_dimension_2d: 2,
        ..offered
    }) else {
        return;
    };
    let made = GpuRenderer::new(device.clone(), room().assets()).err();
    assert_eq!(made, Some(GpuError::TooLarge("the lightmap")));
    assert_eq!(device.take_error(), None);
}

/// The stock materials are 256 texels a side or more, larger than this GPU's
/// textures, while the room's atlas is far smaller.
#[test]
fn a_material_past_the_largest_texture_is_an_error_naming_it() {
    let Some(device) = smaller_gpu(|offered| wgpu::Limits {
        max_texture_dimension_2d: 64,
        ..offered
    }) else {
        return;
    };
    let made = GpuRenderer::new(device.clone(), room().assets()).err();
    assert_eq!(made, Some(GpuError::TooLarge("a material's texture")));
    assert_eq!(device.take_error(), None);
}

#[test]
fn faces_past_a_storage_binding_are_an_error_naming_it() {
    let Some(device) = smaller_gpu(|offered| wgpu::Limits {
        max_storage_buffer_binding_size: 256,
        ..offered
    }) else {
        return;
    };
    let made = GpuRenderer::new(device.clone(), room().assets()).err();
    assert_eq!(made, Some(GpuError::TooLarge("a storage buffer")));
    assert_eq!(device.take_error(), None);
}

/// Actors come from the game, not the level, and as many as it likes.
#[test]
fn more_actors_than_a_buffer_holds_are_an_error_and_the_next_frame_draws() {
    const MOST: u64 = 1 << 20;
    let Some(device) = smaller_gpu(|offered| wgpu::Limits {
        max_buffer_size: MOST,
        ..offered
    }) else {
        return;
    };
    let level = room();
    let mut renderer = GpuRenderer::new(device, level.assets()).expect("fits");
    let one = Actor {
        kind: ActorKind::Enemy,
        pos: Vec3::new(5.0, 5.0, 1.5),
        yaw: 0.0,
    };
    let crowd = vec![one; 20_000];
    assert!(!draw(&mut renderer, &level, (64, 48), &crowd));
    let why = renderer.take_error().expect("a reason");
    assert!(why.contains("the actors"), "{why}");
    assert!(draw(&mut renderer, &level, (64, 48), &[one]), "one fits");
    assert_eq!(renderer.take_error(), None);
}
