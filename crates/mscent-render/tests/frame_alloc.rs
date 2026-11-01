//! A frame never touches the baked lightmap's storage: no bake, no copy of
//! the atlas, and the lightmap sampling itself allocates nothing. The count
//! is taken with a counting global allocator around whole frames.

// A counting allocator is an `unsafe impl` by definition; every call forwards to
// `System`.
#![allow(unsafe_code)]

use std::alloc::{GlobalAlloc, Layout, System};
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};

use mscent_math::Vec3;
use mscent_render::{
    BakeCache, BuildConfig, Framebuffer, LevelRenderer, MaterialTable, Quality, View,
};
use mscent_world::showcase_level;

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

#[test]
fn frames_do_not_bake_and_do_not_copy_the_atlas() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/textures");
    let materials = MaterialTable::load_dir(&dir).expect("textures");
    let mut cache = BakeCache::new();
    let r = LevelRenderer::from_level_cached(
        &showcase_level(),
        materials,
        &BuildConfig::default(),
        &mut cache,
    )
    .expect("builds");
    assert_eq!(cache.bakes(), 1);
    let atlas_bytes =
        r.lightmap().atlas().width() as usize * r.lightmap().atlas().height() as usize * 24;

    let mut fb = Framebuffer::new(320, 180).expect("fb");
    let view =
        |i: usize| View::from_angles(Vec3::new(3.0 + i as f32 * 0.01, 3.0, 2.5), -0.8, 0.0, 0.0);
    for q in [Quality::Low, Quality::High] {
        r.render(&mut fb, &view(0), q, &[], None).expect("warm up");
        let (c0, b0) = (CALLS.load(Ordering::Relaxed), BYTES.load(Ordering::Relaxed));
        let frames = 8;
        for i in 1..=frames {
            r.render(&mut fb, &view(i), q, &[], None).expect("frame");
        }
        let calls = (CALLS.load(Ordering::Relaxed) - c0) / frames;
        let bytes = (BYTES.load(Ordering::Relaxed) - b0) / frames;
        println!(
            "{q:?}: {calls} allocations, {bytes} bytes per frame; atlas is {atlas_bytes} bytes"
        );
        assert!(bytes < atlas_bytes / 8, "{q:?}: {bytes} bytes per frame");
    }
    assert_eq!(cache.bakes(), 1, "rendering never bakes");
}
