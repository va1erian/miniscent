//! Dynamic lights allocate nothing a frame without them does not: the count
//! is taken with a counting global allocator around whole frames.

// A counting allocator is an `unsafe impl` by definition; every call forwards to
// `System`.
#![allow(unsafe_code)]

use std::alloc::{GlobalAlloc, Layout, System};
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};

use mscent_math::{Vec3, Vec3d};
use mscent_render::{
    Actor, ActorKind, BuildConfig, DynLight, Effects, Framebuffer, LevelRenderer, MaterialTable,
    Quality, View,
};
use mscent_world::{Brush, Entity, Level, Op};

struct Counting;

static CALLS: AtomicUsize = AtomicUsize::new(0);

// SAFETY: every method forwards to `System` with the same arguments, so the
// allocator contract is exactly System's; the counter is an atomic.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        CALLS.fetch_add(1, Ordering::Relaxed);
        // SAFETY: forwarded unchanged.
        unsafe { System.alloc(l) }
    }

    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        // SAFETY: forwarded unchanged.
        unsafe { System.dealloc(p, l) }
    }

    unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 {
        CALLS.fetch_add(1, Ordering::Relaxed);
        // SAFETY: forwarded unchanged.
        unsafe { System.realloc(p, l, n) }
    }
}

#[global_allocator]
static ALLOC: Counting = Counting;

fn room() -> LevelRenderer {
    let d = |x, y, z| Vec3d::new(x, y, z);
    let brush = Brush::cuboid(d(-20.0, -20.0, -20.0), d(20.0, 20.0, 20.0), Op::Subtract, 0)
        .expect("a room");
    let level = Level {
        kinds: Vec::new(),
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
        luxel_size: 1.0,
        shadow_samples: 1,
        ..BuildConfig::default()
    };
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/textures");
    let materials = MaterialTable::load_dir(&dir).expect("textures");
    let mut r = LevelRenderer::from_level(&level, materials, &cfg).expect("a level");
    r.set_tiling(24, 1);
    r
}

fn view() -> View {
    View::from_angles(
        Vec3::new(-4.0, 0.0, 0.0),
        -core::f32::consts::FRAC_PI_2,
        0.0,
        0.0,
    )
}

/// Allocations of one steady-state frame of `draw`, after a warm-up frame.
fn calls_per_frame(mut draw: impl FnMut()) -> usize {
    draw();
    let before = CALLS.load(Ordering::Relaxed);
    draw();
    CALLS.load(Ordering::Relaxed) - before
}

#[test]
fn lights_allocate_nothing_a_frame_without_them_does_not() {
    let r = room();
    let mut fb = Framebuffer::new(160, 90).expect("a target");
    let actors = [Actor {
        kind: ActorKind::Crate,
        pos: Vec3::new(2.0, 0.0, 0.0),
        yaw: 0.0,
    }];
    let plain = calls_per_frame(|| {
        r.render_with_particles(&mut fb, &view(), Quality::High, &actors, None, None)
            .expect("a frame");
    });
    println!("allocations per frame, no lights: {plain}");

    let mut light = DynLight::new();
    assert!(light.place(
        r.scene(),
        Vec3::new(2.0, 0.0, 0.0),
        10.0,
        Vec3::new(1.0, 1.0, 1.0)
    ));
    // The second one is close to the far wall, so faces take lights too.
    let mut wall = DynLight::new();
    assert!(wall.place(
        r.scene(),
        Vec3::new(17.0, 0.0, 0.0),
        8.0,
        Vec3::new(1.0, 1.0, 1.0)
    ));
    let lights = [light, wall];
    let lit = calls_per_frame(|| {
        let effects = Effects {
            particles: None,
            lights: &lights,
            poses: &[],
            ..Effects::default()
        };
        r.render_with_effects(&mut fb, &view(), Quality::High, &actors, None, &effects)
            .expect("a frame");
    });
    println!("allocations per frame, two lights: {lit}");
    assert_eq!(
        lit, plain,
        "a light on walls and an actor costs no allocation"
    );

    // Dark lights take the old path whole.
    let dark = [DynLight::new(), DynLight::new()];
    let idle = calls_per_frame(|| {
        let effects = Effects {
            particles: None,
            lights: &dark,
            poses: &[],
            ..Effects::default()
        };
        r.render_with_effects(&mut fb, &view(), Quality::High, &actors, None, &effects)
            .expect("a frame");
    });
    assert_eq!(idle, plain);

    // Placing another light into a light that has held one reuses its sets.
    let mut again = DynLight::new();
    let (pos, white) = (Vec3::new(-3.0, 1.0, 0.0), Vec3::new(1.0, 1.0, 1.0));
    assert!(again.place(r.scene(), pos, 10.0, white));
    let before = CALLS.load(Ordering::Relaxed);
    assert!(again.place(r.scene(), Vec3::new(-2.0, 1.0, 0.0), 10.0, white));
    assert_eq!(CALLS.load(Ordering::Relaxed), before, "a second place");
}
