//! Texture directories on disk: the manifest decides the ids, forged files come
//! back as errors, and adding a material changes no existing pixel.

use std::path::{Path, PathBuf};

use mscent_math::Vec3;
use mscent_render::manifest::{LineError, ManifestError};
use mscent_render::png_encode::encode_png;
use mscent_render::{
    BakeCache, BuildConfig, BuildError, Framebuffer, LevelRenderer, MaterialId, MaterialTable,
    Quality, TextureError, View,
};
use mscent_world::{Brush, Entity, Level, Op, showcase_level};

const STOCK: &str = "0 brick 25\n1 floor 90\n2 lightpanel 120\n3 metal 200\n4 rock 10\n";

fn stock_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/textures")
}

/// A directory that goes away with the test.
struct Fixture(PathBuf);

impl Fixture {
    fn new(name: &str) -> Self {
        let dir =
            std::env::temp_dir().join(format!("mscent_materials_{}_{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("dir");
        Self(dir)
    }

    /// With copies of the stock PNGs and manifest `text`.
    fn with_stock(name: &str, text: &str) -> Self {
        let f = Self::new(name);
        for entry in std::fs::read_dir(stock_dir()).expect("stock") {
            let entry = entry.expect("entry");
            if entry.file_name().to_string_lossy().ends_with(".png") {
                std::fs::copy(entry.path(), f.0.join(entry.file_name())).expect("copy");
            }
        }
        f.write("materials.txt", text.as_bytes());
        f
    }

    fn write(&self, file: &str, bytes: &[u8]) {
        std::fs::write(self.0.join(file), bytes).expect("write");
    }

    fn png(&self, file: &str, side: u32, color: u32) {
        let px = vec![color; (side * side) as usize];
        self.write(file, &encode_png(side, side, &px).expect("encodes"));
    }

    fn load(&self) -> Result<MaterialTable, TextureError> {
        MaterialTable::load_dir(&self.0)
    }

    fn manifest_error(&self) -> ManifestError {
        match self.load() {
            Err(TextureError::Manifest(why)) => why,
            other => panic!("{:?}", other.map(|t| t.len())),
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A PNG with an alpha channel, which `encode_png` does not write: stored
/// deflate blocks, like it.
fn rgba_png(side: u32, argb: u32) -> Vec<u8> {
    let crc = |parts: &[&[u8]]| {
        let mut c = !0u32;
        for &b in parts.iter().copied().flatten() {
            c ^= u32::from(b);
            for _ in 0..8 {
                c = if c & 1 != 0 {
                    0xedb8_8320 ^ c >> 1
                } else {
                    c >> 1
                };
            }
        }
        !c
    };
    let chunk = |out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]| {
        out.extend((data.len() as u32).to_be_bytes());
        out.extend(kind);
        out.extend(data);
        out.extend(crc(&[kind, data]).to_be_bytes());
    };
    let [a, r, g, b] = argb.to_be_bytes();
    let row: Vec<u8> = std::iter::once(0) // filter: none
        .chain((0..side).flat_map(|_| [r, g, b, a]))
        .collect();
    let raw = row.repeat(side as usize);
    let (mut s1, mut s2) = (1u32, 0u32);
    for &x in &raw {
        s1 = (s1 + u32::from(x)) % 65_521;
        s2 = (s2 + s1) % 65_521;
    }
    let mut z = vec![0x78, 0x01, 1];
    z.extend((raw.len() as u16).to_le_bytes());
    z.extend((!(raw.len() as u16)).to_le_bytes());
    z.extend(&raw);
    z.extend((s2 << 16 | s1).to_be_bytes());
    let mut out = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
    let mut ihdr = Vec::new();
    ihdr.extend(side.to_be_bytes());
    ihdr.extend(side.to_be_bytes());
    ihdr.extend([8, 6, 0, 0, 0]);
    chunk(&mut out, b"IHDR", &ihdr);
    chunk(&mut out, b"IDAT", &z);
    chunk(&mut out, b"IEND", &[]);
    out
}

#[test]
fn a_directory_with_no_manifest_gets_sorted_ids_and_the_default_gloss() {
    let f = Fixture::new("listing");
    f.png("b.png", 2, 0xff00_0000);
    f.png("a.png", 2, 0xff00_0000);
    f.png("a_n.png", 2, 0xff80_80ff);
    f.png("_sheet.png", 2, 0xff00_0000);
    let t = f.load().expect("loads");
    let names: Vec<(u16, &str, u8)> = t
        .iter()
        .map(|(i, m)| (i.0, m.name.as_str(), m.gloss))
        .collect();
    assert_eq!(names, [(0, "a", 40), (1, "b", 40)]);
}

#[test]
fn a_png_the_manifest_does_not_list_is_ignored_and_shifts_nothing() {
    let f = Fixture::with_stock("unlisted", STOCK);
    f.png("aaa.png", 8, 0xffff_0000);
    f.png("concrete.png", 8, 0xffff_0000);
    let t = f.load().expect("loads");
    assert_eq!(t.len(), 5);
    assert_eq!(t.find("brick"), Some(MaterialId(0)));
    assert_eq!(t.find("aaa"), None);
    assert_eq!(t.find("concrete"), None);
}

#[test]
fn a_listed_material_with_no_png_is_an_error_unless_retired() {
    let f = Fixture::with_stock("missing", &format!("{STOCK}5 concrete 40\n"));
    assert!(matches!(f.load(), Err(TextureError::Io(_))));
    f.write(
        "materials.txt",
        format!("{STOCK}5 retired\n6 rock2 3\n").as_bytes(),
    );
    f.png("rock2.png", 8, 0xffff_0000);
    let t = f.load().expect("loads");
    assert_eq!(t.len(), 7);
    assert!(t.get(MaterialId(5)).is_none());
    assert_eq!(t.find("rock2"), Some(MaterialId(6)));
    assert_eq!(t.get(MaterialId(6)).expect("there").gloss, 3);
}

#[test]
fn forged_manifests_are_errors() {
    let f = Fixture::with_stock("forged", STOCK);
    let bad = |text: &str| {
        f.write("materials.txt", text.as_bytes());
        f.manifest_error()
    };
    let line = |n, why| ManifestError::Line(n, why);
    assert_eq!(bad(""), ManifestError::Empty);
    assert_eq!(bad("0 ../../etc/passwd 1\n"), line(1, LineError::BadName));
    assert_eq!(
        bad("0 brick 25\n0 brick 25\n"),
        line(2, LineError::DuplicateId)
    );
    assert_eq!(
        bad("0 brick 25\n1 brick 25\n"),
        line(2, LineError::DuplicateName)
    );
    assert_eq!(bad("0 brick 25\n5 rock 10\n"), line(2, LineError::Gap));
    assert_eq!(bad("0 retired\n"), line(1, LineError::RetiredStock));
    assert_eq!(bad("0 brick 256\n"), line(1, LineError::BadGloss));
    assert_eq!(bad("70000 brick 1\n"), line(1, LineError::BadId));
    f.write("materials.txt", &[0xff, 0xfe, 0x00, 0x41]);
    assert_eq!(f.manifest_error(), ManifestError::NotText);
    f.write("materials.txt", &vec![b'#'; 64 * 1024 + 1]);
    assert_eq!(f.manifest_error(), ManifestError::TooLarge);
    // A file that cannot be a manifest fails closed: no listing behind its back.
    std::fs::remove_file(f.0.join("materials.txt")).expect("removed");
    std::fs::create_dir(f.0.join("materials.txt")).expect("a directory");
    assert!(matches!(f.load(), Err(TextureError::Io(_))));
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

#[test]
fn forged_specular_files_are_errors() {
    let f = Fixture::with_stock("forged_s", STOCK);
    f.png("brick_s.png", 64, 0xff00_0000);
    assert!(
        matches!(f.load(), Err(TextureError::SizeMismatch)),
        "smaller"
    );
    f.png("brick_s.png", 256, 0xff00_0000);
    assert!(
        matches!(f.load(), Err(TextureError::SizeMismatch)),
        "larger"
    );
    f.write("brick_s.png", b"not a png");
    assert!(matches!(f.load(), Err(TextureError::Png(_))));
    f.write("brick_s.png", b"");
    assert!(matches!(f.load(), Err(TextureError::Png(_))));
    let mut truncated = encode_png(128, 128, &vec![0xff00_0000; 128 * 128]).expect("encodes");
    truncated.truncate(truncated.len() / 2);
    f.write("brick_s.png", &truncated);
    assert!(matches!(f.load(), Err(TextureError::Png(_))));
    std::fs::remove_file(f.0.join("brick_s.png")).expect("removed");
    assert!(f.load().is_ok(), "the fixture was fine all along");
}

#[test]
fn the_first_channel_of_a_specular_file_is_the_mask() {
    let f = Fixture::new("channel");
    f.write("materials.txt", b"0 a 9\n");
    f.png("a.png", 2, 0xff00_0000);
    f.png("a_s.png", 2, 0xff40_ffff);
    let t = f.load().expect("loads");
    let tex = &t.get(MaterialId(0)).expect("there").texture;
    assert_eq!(tex.normal_texel(0, 0).expect("flat") >> 24, 0x40);
    assert_eq!(tex.spec_mean(), f32::from(0x40u8) / 255.0);
}

#[test]
fn the_alpha_of_a_normal_map_file_is_no_mask() {
    let f = Fixture::new("alpha");
    f.write("materials.txt", b"0 a 9\n");
    f.png("a.png", 4, 0xff00_0000);
    f.write("a_n.png", &rgba_png(4, 0x1280_80ff));
    let t = f.load().expect("loads");
    let tex = &t.get(MaterialId(0)).expect("there").texture;
    assert_eq!(tex.normal_texel(1, 1), Some(0xff80_80ff));
    assert_eq!(tex.spec_mean(), 1.0);
    // With a specular file the file decides, not the normal map.
    f.png("a_s.png", 4, 0xff7f_0000);
    let t = f.load().expect("loads");
    let tex = &t.get(MaterialId(0)).expect("there").texture;
    assert_eq!(tex.normal_texel(1, 1), Some(0x7f80_80ff));
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
    let text = format!("{STOCK}5 aaa 200\n6 retired\n7 zzz 40\n");
    let f = Fixture::with_stock("added", &text);
    f.png("aaa.png", 128, 0xffff_00ff);
    f.png("aaa_s.png", 128, 0xff40_4040);
    f.png("zzz.png", 64, 0xff00_ff00);
    let bigger = f.load().expect("loads");
    let stock = MaterialTable::load_dir(&stock_dir()).expect("stock");
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
    let f = Fixture::with_stock("full", STOCK);
    f.png("brick_s.png", 128, 0xffff_ffff);
    f.png("metal_s.png", 128, 0xffff_ffff);
    let level = showcase_level();
    let cfg = BuildConfig::default();
    let mut cache = BakeCache::new();
    let eye = Vec3::new(6.0, 6.0, 3.0);
    let stock = MaterialTable::load_dir(&stock_dir()).expect("stock");
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
    let f = Fixture::with_stock("new_id", &format!("{STOCK}5 concrete 40\n6 retired\n"));
    f.png("concrete.png", 128, 0xff00_ff00);
    let green = room_picture(&f, 5, Quality::Low);
    let (r, g) = green.iter().fold((0u64, 0u64), |(r, g), p| {
        (r + u64::from(p >> 16 & 0xff), g + u64::from(p >> 8 & 0xff))
    });
    assert!(g > r * 5, "id 5 is green: {g} against {r}");

    let grey = room_picture(&f, 6, Quality::High);
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
    let f = Fixture::with_stock("dull", &format!("{STOCK}5 dull 0\n"));
    f.png("dull.png", 128, 0xff80_8080);
    std::fs::copy(stock_dir().join("brick_n.png"), f.0.join("dull_n.png")).expect("copy");
    let plain = room_picture(&f, 5, Quality::High);
    f.png("dull_s.png", 128, 0xff00_0000);
    assert_eq!(plain, room_picture(&f, 5, Quality::High));
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
    let f = Fixture::with_stock("black", STOCK);
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
