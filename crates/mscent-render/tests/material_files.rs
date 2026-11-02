//! Texture directories on disk: the manifest decides the ids, forged files come
//! back as errors, and companion files (normals, heights, masks) load. The
//! directories are PNG copies of the stock materials, see `material_fixture`.

#[path = "support/material_fixture.rs"]
mod material_fixture;

use material_fixture::{Fixture, STOCK, stock_dir};
use mscent_render::manifest::{LineError, ManifestError};
use mscent_render::png_encode::encode_png;
use mscent_render::{MaterialId, MaterialTable, TextureError};

fn manifest_error(f: &Fixture) -> ManifestError {
    match f.load() {
        Err(TextureError::Manifest(why)) => why,
        other => panic!("{:?}", other.map(|t| t.len())),
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
        manifest_error(&f)
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
    assert_eq!(manifest_error(&f), ManifestError::NotText);
    f.write("materials.txt", &vec![b'#'; 64 * 1024 + 1]);
    assert_eq!(manifest_error(&f), ManifestError::TooLarge);
    // A file that cannot be a manifest fails closed: no listing behind its back.
    std::fs::remove_file(f.0.join("materials.txt")).expect("removed");
    std::fs::create_dir(f.0.join("materials.txt")).expect("a directory");
    assert!(matches!(f.load(), Err(TextureError::Io(_))));
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
fn a_specular_file_with_no_normal_map_is_an_error() {
    let f = Fixture::new("no_normals");
    f.write("materials.txt", b"0 a 9\n");
    f.png("a.png", 2, 0xff00_0000);
    assert!(f.load().is_ok(), "no specular file, no problem");
    f.png("a_s.png", 2, 0xff40_ffff);
    assert!(matches!(
        f.load(),
        Err(TextureError::SpecularWithoutNormals)
    ));
    f.png("a_n.png", 2, 0xff80_80ff);
    assert!(f.load().is_ok(), "the map it needs");
}

#[test]
fn the_first_channel_of_a_specular_file_is_the_mask() {
    let f = Fixture::new("channel");
    f.write("materials.txt", b"0 a 9\n");
    f.png("a.png", 2, 0xff00_0000);
    f.png("a_n.png", 2, 0xff80_80ff);
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

#[test]
fn a_recipe_wins_over_a_png_of_the_same_name() {
    let f = Fixture::with_stock("recipe_wins", STOCK);
    let recipe = std::fs::read(stock_dir().join("brick.mst")).expect("stock recipe");
    let from_recipes = MaterialTable::load_dir(&stock_dir()).expect("stock");
    let brick = &from_recipes.get(MaterialId(0)).expect("there").texture;

    // The PNG is a different picture, and a forged one after that: it must not
    // even be read once the recipe is there.
    f.png("brick.png", 128, 0xff00_ff00);
    f.write("brick.mst", &recipe);
    let t = f.load().expect("loads");
    let tex = &t.get(MaterialId(0)).expect("there").texture;
    assert_eq!(tex.texel(5, 7), brick.texel(5, 7));
    assert_ne!(tex.texel(5, 7), 0xff00_ff00);
    f.write("brick.png", b"not a png");
    assert!(f.load().is_ok(), "a recipe never reads the PNG");

    // Without the recipe the same directory falls back to the PNG.
    std::fs::remove_file(f.0.join("brick.mst")).expect("removed");
    assert!(matches!(f.load(), Err(TextureError::Png(_))));
}
