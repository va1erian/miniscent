//! Pictures drawn from texture directories: adding a material or a mask changes
//! only what it should. The directories are PNG copies of the stock materials,
//! see `material_fixture`.

#[path = "support/material_fixture.rs"]
mod material_fixture;

use material_fixture::{Fixture, STOCK, stock_dir};
use mscent_math::Vec3;
use mscent_render::{
    BakeCache, BuildConfig, BuildError, Framebuffer, LevelRenderer, MaterialTable, Quality, View,
};
use mscent_world::{Brush, Entity, Level, Op, showcase_level};

#[test]
fn stock_names_with_no_manifest_keep_their_gloss_and_draw_the_same() {
    let listed = Fixture::with_stock("no_manifest", STOCK);
    std::fs::remove_file(listed.0.join("materials.txt")).expect("removed");
    let t = listed.load().expect("loads");
    let got: Vec<(u16, &str, u8)> = t
        .iter()
        .map(|(i, m)| (i.0, m.name.as_str(), m.gloss))
        .collect();
    assert_eq!(
        got,
        [
            (0, "brick", 25),
            (1, "floor", 90),
            (2, "lightpanel", 120),
            (3, "metal", 200),
            (4, "rock", 10),
            (5, "bark", 8),
            (6, "cap", 70),
            (7, "coral", 30),
            (8, "stem", 20),
            (9, "wax", 90),
        ]
    );
    let stock = MaterialTable::load_dir(&stock_dir()).expect("stock");
    let cfg = light_cfg();
    let mut cache = BakeCache::new();
    let level = room(3);
    let a = LevelRenderer::from_level_cached(&level, t, &cfg, &mut cache).expect("builds");
    let b = LevelRenderer::from_level_cached(&level, stock, &cfg, &mut cache).expect("builds");
    assert_eq!(
        pictures(&a, Vec3::new(0.0, -2.0, 0.0)),
        pictures(&b, Vec3::new(0.0, -2.0, 0.0))
    );
}

#[test]
fn a_forged_stock_order_stops_the_level_with_the_first_wrong_id() {
    let f = Fixture::with_stock(
        "swapped",
        "0 floor 90\n1 brick 25\n2 lightpanel 120\n3 metal 200\n4 rock 10\n",
    );
    let table = f.load().expect("a fine manifest");
    let got = LevelRenderer::from_level(&room(0), table, &light_cfg());
    assert_eq!(got.err(), Some(BuildError::Materials(0)));
}

fn light_cfg() -> BuildConfig {
    BuildConfig {
        luxel_size: 0.5,
        shadow_samples: 1,
        ..BuildConfig::default()
    }
}

/// A lit room of one material.
fn room(material: u16) -> Level {
    let d = mscent_math::Vec3d::new;
    let brush = Brush::cuboid(d(-6.0, -6.0, -3.0), d(6.0, 6.0, 3.0), Op::Subtract, 0)
        .expect("room")
        .with_material(material);
    Level {
        brushes: vec![brush],
        entities: vec![Entity::Light {
            pos: Vec3::new(0.0, 0.0, 1.0),
            color: Vec3::new(1.0, 1.0, 1.0),
            radius: 20.0,
            intensity: 1.0,
        }],
        ..Default::default()
    }
}

/// Pictures at every quality from around a level.
fn pictures(r: &LevelRenderer, eye: Vec3) -> Vec<u32> {
    let mut fb = Framebuffer::new(256, 144).expect("fb");
    let mut all = Vec::new();
    for q in [Quality::Low, Quality::Medium, Quality::High] {
        for i in 0..5 {
            let view = View::from_angles(eye, i as f32 * 1.3, -0.2, 0.0);
            r.render(&mut fb, &view, q, r.actors(), None)
                .expect("renders");
            all.extend_from_slice(fb.pixels());
        }
    }
    all
}

#[test]
fn adding_a_material_leaves_stock_ids_and_the_pixels_of_a_level_unchanged() {
    // `aaa` sorts before every stock name: with ids from the file list it would
    // move them all, and the level would not even build.
    let text = format!("{STOCK}10 aaa 200\n11 retired\n12 zzz 40\n");
    let f = Fixture::with_stock("added", &text);
    f.png("aaa.png", 128, 0xffff_00ff);
    f.png("aaa_n.png", 128, 0xff80_80ff);
    f.png("aaa_s.png", 128, 0xff40_4040);
    f.png("zzz.png", 64, 0xff00_ff00);
    let bigger = f.load().expect("loads");
    let stock = Fixture::with_stock("added_base", STOCK)
        .load()
        .expect("stock");
    for (id, m) in stock.iter() {
        let other = bigger.get(id).expect("kept");
        assert_eq!(
            (other.name.as_str(), other.gloss),
            (m.name.as_str(), m.gloss)
        );
    }

    let (level, eye) = (showcase_level(), Vec3::new(6.0, 6.0, 3.0));
    let cfg = BuildConfig::default();
    let mut cache = BakeCache::new();
    let before = LevelRenderer::from_level_cached(&level, stock, &cfg, &mut cache).expect("builds");
    let after = LevelRenderer::from_level_cached(&level, bigger, &cfg, &mut cache).expect("builds");
    assert_eq!(pictures(&before, eye), pictures(&after, eye));
    assert_eq!(cache.bakes(), 1, "materials are no part of the light");
}

#[test]
fn a_full_specular_file_changes_no_pixel_of_a_stock_material() {
    // Metal ships with a mask now: take it out, so that "no mask" is what the
    // full one is compared with.
    let plain = Fixture::with_stock("full_plain", STOCK);
    std::fs::remove_file(plain.0.join("metal_s.png")).expect("removed");
    let f = Fixture::with_stock("full", STOCK);
    std::fs::remove_file(f.0.join("metal_s.png")).expect("removed");
    f.png("brick_s.png", 128, 0xffff_ffff);
    f.png("metal_s.png", 128, 0xffff_ffff);
    let level = showcase_level();
    let cfg = BuildConfig::default();
    let mut cache = BakeCache::new();
    let eye = Vec3::new(6.0, 6.0, 3.0);
    let stock = plain.load().expect("stock");
    let plain = LevelRenderer::from_level_cached(&level, stock, &cfg, &mut cache).expect("builds");
    let full = f.load().expect("loads");
    let full = LevelRenderer::from_level_cached(&level, full, &cfg, &mut cache).expect("builds");
    assert_eq!(pictures(&plain, eye), pictures(&full, eye));
}

/// The picture of a room of `material`, with the materials of `dir`.
fn room_picture(dir: &Fixture, material: u16, q: Quality) -> Vec<u32> {
    let t = dir.load().expect("loads");
    let r = LevelRenderer::from_level(&room(material), t, &light_cfg()).expect("builds");
    let mut fb = Framebuffer::new(256, 144).expect("fb");
    let view = View::from_angles(Vec3::new(0.0, -2.0, 0.0), 0.0, 0.0, 0.0);
    r.render(&mut fb, &view, q, &[], None).expect("renders");
    fb.pixels().to_vec()
}

#[test]
fn a_new_id_draws_its_own_material_and_a_retired_one_the_grey_fallback() {
    let f = Fixture::with_stock("new_id", &format!("{STOCK}10 concrete 40\n11 retired\n"));
    f.png("concrete.png", 128, 0xff00_ff00);
    let green = room_picture(&f, 10, Quality::Low);
    let (r, g) = green.iter().fold((0u64, 0u64), |(r, g), p| {
        (r + u64::from(p >> 16 & 0xff), g + u64::from(p >> 8 & 0xff))
    });
    assert!(g > r * 5, "id 10 is green: {g} against {r}");

    let grey = room_picture(&f, 11, Quality::High);
    let (mut r, mut g) = (0u64, 0u64);
    for p in &grey {
        r += u64::from(p >> 16 & 0xff);
        g += u64::from(p >> 8 & 0xff);
    }
    assert!(g > 0 && r.abs_diff(g) * 8 < g, "id 6 is grey: {r} and {g}");
}

#[test]
fn a_mask_on_a_material_too_dull_for_a_highlight_changes_nothing() {
    // Gloss 0 falls back to the lit shader, which never looks at the mask.
    let f = Fixture::with_stock("dull", &format!("{STOCK}10 dull 0\n"));
    f.png("dull.png", 128, 0xff80_8080);
    std::fs::copy(f.0.join("brick_n.png"), f.0.join("dull_n.png")).expect("copy");
    let plain = room_picture(&f, 10, Quality::High);
    f.png("dull_s.png", 128, 0xff00_0000);
    assert_eq!(plain, room_picture(&f, 10, Quality::High));
}

#[test]
fn a_mask_on_the_emissive_panel_changes_nothing() {
    // The lightpanel draws flat, with no highlight to mask.
    let f = Fixture::with_stock("panel", STOCK);
    let plain = room_picture(&f, 2, Quality::High);
    f.png("lightpanel_s.png", 128, 0xff00_0000);
    assert_eq!(plain, room_picture(&f, 2, Quality::High));
}

#[test]
fn a_real_mask_changes_the_level_and_only_at_high_quality() {
    // Metal is glossy; a black mask takes its highlights away.
    // Unmasked first: metal ships with a mask of its own.
    let f = Fixture::with_stock("black", STOCK);
    std::fs::remove_file(f.0.join("metal_s.png")).expect("removed");
    let (low, medium, high) = (
        room_picture(&f, 3, Quality::Low),
        room_picture(&f, 3, Quality::Medium),
        room_picture(&f, 3, Quality::High),
    );
    f.png("metal_s.png", 128, 0xff00_0000);
    assert_eq!(low, room_picture(&f, 3, Quality::Low));
    assert_eq!(medium, room_picture(&f, 3, Quality::Medium));
    let masked = room_picture(&f, 3, Quality::High);
    assert_eq!(masked, medium, "no highlight is left: High draws as Medium");
    assert_ne!(masked, high);
}
