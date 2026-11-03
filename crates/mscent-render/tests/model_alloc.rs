//! A frame with models allocates no more than one without. Alone in its own binary,
//! as the counter counts every thread: another test loading a level would show here.

// A counting allocator is an `unsafe impl` by definition; every call forwards to
// `System`.
#![allow(unsafe_code)]

use std::alloc::{GlobalAlloc, Layout, System};
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};

use mscent_math::{Quat, Vec3};
use mscent_render::model::{ModelId, ModelInstance, ModelTable};
use mscent_render::{
    BuildConfig, Effects, Framebuffer, LevelRenderer, MaterialTable, Quality, View,
};
use mscent_world::model::{build, read};
use mscent_world::showcase_level;

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

const SIZE: (u32, u32) = (320, 180);
const STOCK: [&str; 3] = [
    include_str!("../../../assets/enemies/sentry.msb"),
    include_str!("../../../assets/enemies/dart.msb"),
    include_str!("../../../assets/enemies/mantis.msb"),
];

fn materials() -> MaterialTable {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/textures");
    MaterialTable::load_dir(&dir).expect("textures")
}

fn renderer() -> LevelRenderer {
    let cfg = BuildConfig {
        shadow_samples: 1,
        luxel_size: 1.0,
        ..BuildConfig::default()
    };
    LevelRenderer::from_level(&showcase_level(), materials(), &cfg).expect("builds")
}

fn table() -> (ModelTable, Vec<ModelId>) {
    let mats = materials();
    let mut table = ModelTable::new();
    let ids = STOCK
        .iter()
        .map(|text| {
            let blueprint = read(text.as_bytes()).expect("reads");
            let model = build(&blueprint).expect("builds");
            table.add(&blueprint, &model, &mats)
        })
        .collect();
    (table, ids)
}

/// A view into the showcase, with a model of each design in front of it.
fn scene(ids: &[ModelId], at: f32) -> (View, Vec<ModelInstance>) {
    let eye = Vec3::new(3.0 + at * 0.01, 3.0, 2.5);
    let view = View::from_angles(eye, -0.8, 0.0, 0.0);
    let ahead = view.forward();
    let across = Vec3::new(ahead.y, -ahead.x, 0.0);
    let spin = Quat::from_axis_angle(Vec3::new(0.0, 0.0, 1.0), at * 0.1);
    let instances = ids
        .iter()
        .zip([-2.0f32, 0.0, 2.0])
        .map(|(&id, side)| ModelInstance::new(id, eye + ahead * 4.0 + across * side, spin))
        .collect();
    (view, instances)
}

fn frame(
    r: &LevelRenderer,
    fb: &mut Framebuffer,
    view: &View,
    models: Option<(&ModelTable, &[ModelInstance])>,
) -> u32 {
    let effects = Effects {
        models,
        ..Effects::default()
    };
    let stats = r
        .render_with_effects(fb, view, Quality::High, r.actors(), None, &effects)
        .expect("frame");
    stats.model_triangles
}

#[test]
fn a_frame_with_models_allocates_no_more_than_one_without() {
    let r = renderer();
    let (table, ids) = table();
    let mut fb = Framebuffer::new(SIZE.0, SIZE.1).expect("fb");
    let mut count = |models: bool| {
        for warm in 0..4 {
            let (view, instances) = scene(&ids, warm as f32);
            let held = models.then_some((&table, instances.as_slice()));
            frame(&r, &mut fb, &view, held);
        }
        let frames = 8;
        let mut total = 0;
        for i in 0..frames {
            // The instance list is the game's, made outside the frame.
            let (view, instances) = scene(&ids, 4.0 + i as f32);
            let held = models.then_some((&table, instances.as_slice()));
            let before = CALLS.load(Ordering::Relaxed);
            frame(&r, &mut fb, &view, held);
            total += CALLS.load(Ordering::Relaxed) - before;
        }
        total / frames
    };
    let without = count(false);
    let with = count(true);
    println!("{without} allocations a frame without models, {with} with");
    // The same views: the level's polygon lists do the same work in both. A few
    // for lists still finding their size is not one per triangle or per band.
    assert!(with <= without + 8, "{with} against {without}");
}
