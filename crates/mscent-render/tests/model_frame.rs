//! Models in whole frames: they draw through the level's own pass, and the picture
//! does not depend on the number of bands.

use std::path::Path;

use mscent_math::{Quat, Vec3};
use mscent_render::model::{ModelId, ModelInstance, ModelTable};
use mscent_render::{
    BuildConfig, Effects, Framebuffer, LevelRenderer, MaterialTable, Quality, View,
};
use mscent_world::model::{build, read};
use mscent_world::showcase_level;

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
fn models_change_the_picture_and_no_models_change_nothing() {
    let r = renderer();
    let (table, ids) = table();
    let (view, instances) = scene(&ids, 0.0);
    let mut plain = Framebuffer::new(SIZE.0, SIZE.1).expect("fb");
    assert_eq!(frame(&r, &mut plain, &view, None), 0);
    let mut empty = Framebuffer::new(SIZE.0, SIZE.1).expect("fb");
    assert_eq!(frame(&r, &mut empty, &view, Some((&table, &[]))), 0);
    assert_eq!(plain.pixels(), empty.pixels());
    let mut with = Framebuffer::new(SIZE.0, SIZE.1).expect("fb");
    let triangles = frame(&r, &mut with, &view, Some((&table, &instances)));
    assert!(triangles > 500, "{triangles} triangles");
    let changed = plain
        .pixels()
        .iter()
        .zip(with.pixels())
        .filter(|(a, b)| a != b)
        .count();
    assert!(changed > 500, "{changed} pixels changed");
}

#[test]
fn the_picture_does_not_depend_on_the_bands() {
    let (table, ids) = table();
    let (view, instances) = scene(&ids, 3.0);
    let shot = |rows: u32, workers: usize| {
        let mut r = renderer();
        r.set_tiling(rows, workers);
        let mut fb = Framebuffer::new(SIZE.0, SIZE.1).expect("fb");
        frame(&r, &mut fb, &view, Some((&table, &instances)));
        fb.pixels().to_vec()
    };
    assert!(shot(SIZE.1, 1) == shot(24, 4), "the pictures differ");
}
