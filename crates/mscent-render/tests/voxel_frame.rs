//! A level with voxel surfaces costs no more allocations per frame than the walk
//! it shares with every level, once the first frame has grown the lists.

// A counting allocator is an `unsafe impl` by definition; every call forwards to
// `System`.
#![allow(unsafe_code)]

use std::alloc::{GlobalAlloc, Layout, System};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use mscent_math::{Vec3, Vec3d};
use mscent_render::{BuildConfig, Compiled, Framebuffer, MaterialTable, Quality, View};
use mscent_world::voxel::{CellSize, Context, Volume, fill};
use mscent_world::{Entity, Level, Op};

struct Counting;

static CALLS: AtomicUsize = AtomicUsize::new(0);

// SAFETY: every method forwards to `System` with the same arguments, so the
// allocator contract is exactly System's; the counter is atomic.
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

/// A frame of a level with no faces: the walk's own vectors, whatever it draws.
const MAX_ALLOCATIONS: usize = 64;

fn gyroid(p: Vec3d) -> f64 {
    let g = |x: f64, y: f64, z: f64| x.sin() * y.cos() + y.sin() * z.cos() + z.sin() * x.cos();
    g(p.x * 0.55, p.y * 0.55, p.z * 0.55) + 0.25
}

#[test]
fn voxel_frames_allocate_nothing_of_their_own() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/textures");
    let materials = MaterialTable::load_dir(&dir).expect("textures");
    let mut volume =
        Volume::new(Op::Subtract, 0, CellSize::Quarter, [0; 3], [4, 4, 2]).expect("valid");
    let shape = volume.clone();
    fill(&mut volume, &mut Context::new(&shape, &[]), gyroid, 4);
    let level = Level {
        volumes: vec![volume],
        entities: vec![Entity::Light {
            pos: Vec3::new(8.0, 8.0, 4.0),
            color: Vec3::new(1.0, 1.0, 1.0),
            radius: 30.0,
            intensity: 1.0,
        }],
        ..Default::default()
    };
    let cfg = BuildConfig {
        luxel_size: 1.0,
        ..BuildConfig::default()
    };
    let c = Compiled::new(&level, &cfg).expect("compiles");
    let map = Arc::new(c.draft().expect("drafts"));
    let mut r = c.into_renderer(map, materials).expect("assembles");
    r.set_tiling(24, 1);
    let mut fb = Framebuffer::new(320, 180).expect("fb");
    let view = View::from_angles(Vec3::new(7.0, 7.0, 3.0), 0.5, 0.0, 0.0);
    for _ in 0..3 {
        r.render(&mut fb, &view, Quality::High, &[], None)
            .expect("renders");
    }
    let before = CALLS.load(Ordering::Relaxed);
    let stats = r
        .render(&mut fb, &view, Quality::High, &[], None)
        .expect("renders");
    let calls = CALLS.load(Ordering::Relaxed) - before;
    assert!(stats.voxel_triangles > 0);
    assert!(calls <= MAX_ALLOCATIONS, "{calls} allocations in a frame");
}
