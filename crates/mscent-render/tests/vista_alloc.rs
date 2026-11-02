//! A vista costs a small fixed number of allocations per layer and frame, and the
//! second depth plane is made once. Its own binary: the counter is global. A level
//! with no viewport pays nothing, which `frame_alloc.rs` keeps.

// A counting allocator is an `unsafe impl` by definition; every call forwards to
// `System`.
#![allow(unsafe_code)]

use std::alloc::{GlobalAlloc, Layout, System};
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};

use mscent_math::{Vec3, Vec3d};
use mscent_render::{
    BuildConfig, Effects, Framebuffer, LevelRenderer, MaterialTable, Quality, View,
};
use mscent_world::materials::{LIGHTPANEL, METAL, SKY_CLOUDS, VIEW_FIRST};
use mscent_world::{Brush, Entity, Level, MAX_VISTA_FOG, Op};

struct Counting;

static CALLS: AtomicUsize = AtomicUsize::new(0);
static BYTES: AtomicUsize = AtomicUsize::new(0);

// SAFETY: every method forwards to `System` with the same arguments, so the
// allocator contract is exactly System's; the counters are atomics.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        CALLS.fetch_add(1, Ordering::Relaxed);
        BYTES.fetch_add(l.size(), Ordering::Relaxed);
        // SAFETY: forwarded unchanged.
        unsafe { System.alloc(l) }
    }

    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        // SAFETY: forwarded unchanged.
        unsafe { System.dealloc(p, l) }
    }

    unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 {
        CALLS.fetch_add(1, Ordering::Relaxed);
        BYTES.fetch_add(n, Ordering::Relaxed);
        // SAFETY: forwarded unchanged.
        unsafe { System.realloc(p, l, n) }
    }
}

#[global_allocator]
static ALLOC: Counting = Counting;

const W: u32 = 160;
const H: u32 = 90;
const AWAY: f64 = 1000.0;
/// Allocations one layer may add to a frame (its lists and flood).
const PER_LAYER: usize = 64;
/// What listing one more face of a layer may cost.
const FACE_ALLOCATIONS: usize = 12;

fn vd(x: f64, y: f64, z: f64) -> Vec3d {
    Vec3d::new(x, y, z)
}

fn cuboid(min: Vec3d, max: Vec3d, op: Op, order: u32, material: u16) -> Brush {
    Brush::cuboid(min, max, op, order)
        .expect("a cuboid")
        .with_material(material)
}

/// A metal play room with a viewport window `half` wide, and `layers` sealed sky
/// rooms behind it, spinning and panning.
fn level(half: f64, layers: u8) -> Level {
    let room = |x: f64, order: u32, material: u16| {
        let (min, max) = (vd(x - 16.0, -16.0, -16.0), vd(x + 16.0, 48.0, 16.0));
        cuboid(min, max, Op::Subtract, order, material)
    };
    let mut brushes = vec![room(0.0, 0, METAL)];
    let window = (vd(-half, 40.0, -half), vd(half, 48.0, half));
    brushes.push(cuboid(window.0, window.1, Op::Add, 1, VIEW_FIRST));
    let mut entities = Vec::new();
    for i in 0..layers {
        let x = AWAY * f64::from(i + 1);
        brushes.push(room(x, 2 + u32::from(i), SKY_CLOUDS));
        let (lo, hi) = (vd(x - 2.0, 30.0, -2.0), vd(x + 2.0, 31.0, 2.0));
        brushes.push(cuboid(lo, hi, Op::Add, 10 + u32::from(i), LIGHTPANEL));
        entities.push(Entity::VistaLayer {
            pos: Vec3::new(x as f32, 0.0, 0.0),
            centre: Vec3::ZERO,
            slot: 0,
            layer: i,
            parallax: 0.0,
            spin: Vec3::new(0.0, 0.0, 0.7),
            pan: [90.0, 40.0],
            fog_start: 4.0,
            fog_end: MAX_VISTA_FOG,
            fog_color: 0,
        });
    }
    Level {
        brushes,
        entities,
        ..Level::default()
    }
}

fn renderer(level: &Level) -> LevelRenderer {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/textures");
    let materials = MaterialTable::load_dir(&dir).expect("textures");
    let cfg = BuildConfig {
        luxel_size: 8.0,
        shadow_samples: 1,
        ..BuildConfig::default()
    };
    LevelRenderer::from_level(level, materials, &cfg).expect("builds")
}

const FRAMES: usize = 8;

/// Allocations and bytes of each of `FRAMES` frames, the time changing every
/// frame, twice over after a first frame: the second run repeats the first's times.
/// (The pan has its storage already in the first frame, as time is above 0.)
fn per_frame(r: &LevelRenderer) -> [Vec<(usize, usize)>; 2] {
    let mut fb = Framebuffer::new(W, H).expect("fb");
    let view = View::from_angles(Vec3::new(0.5, 0.0, 0.0), 0.02, 0.0, 0.0);
    let mut draw = |i: usize| {
        let effects = Effects {
            vista_time: 0.37 * (i + 1) as f32,
            ..Effects::default()
        };
        let (c0, b0) = (CALLS.load(Ordering::Relaxed), BYTES.load(Ordering::Relaxed));
        r.render_with_effects(&mut fb, &view, Quality::Low, &[], None, &effects)
            .expect("a frame");
        let (c1, b1) = (CALLS.load(Ordering::Relaxed), BYTES.load(Ordering::Relaxed));
        (c1 - c0, b1 - b0)
    };
    draw(0);
    std::array::from_fn(|_| (0..FRAMES).map(&mut draw).collect())
}

#[test]
fn a_vista_allocates_a_small_fixed_number_per_layer_and_frame() {
    let mut r = renderer(&level(4.0, 0));
    r.set_tiling(24, 1);
    let bare = per_frame(&r)[1][0].0;
    let depth_plane = (W * H) as usize * 4;
    for layers in 1..=3u8 {
        let mut r = renderer(&level(4.0, layers));
        r.set_tiling(24, 1);
        let [first, second] = per_frame(&r);
        println!("{layers} layers: {second:?} (no layer: {bare})");
        // What a frame allocates depends on what it draws, not on how many frames
        // came before: the same times give the same counts.
        assert_eq!(first, second, "the count grew with the frames");
        for &(calls, bytes) in &second {
            // The plane is made once; a frame that made it again would cost it.
            assert!(bytes < depth_plane, "{bytes} bytes in a frame");
            let extra = calls.saturating_sub(bare);
            assert!(extra <= PER_LAYER * usize::from(layers), "{extra} extra");
        }
    }

    // A hole two and a half times as wide shows more of the layer's room, which
    // lists a few more faces, but allocates nothing for its pixels: the count stays
    // within a face's worth or two of the small hole's, and under the per-layer bound.
    let layer_cost = |half: f64| {
        let counts = |layers| {
            let mut r = renderer(&level(half, layers));
            r.set_tiling(24, 1);
            per_frame(&r)[1].iter().map(|c| c.0).collect::<Vec<_>>()
        };
        let (with, without) = (counts(1), counts(0));
        with.iter()
            .zip(&without)
            .map(|(a, b)| a - b)
            .collect::<Vec<_>>()
    };
    let (small, large) = (layer_cost(4.0), layer_cost(10.0));
    for (s, l) in small.iter().zip(&large) {
        assert!(
            *l <= s + FACE_ALLOCATIONS * 2 && *l <= PER_LAYER,
            "{small:?} {large:?}"
        );
    }
}
