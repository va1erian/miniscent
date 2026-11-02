use std::path::PathBuf;

use super::*;
use crate::texture::TextureError;
use mscent_math::Vec3;

fn textures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets/textures")
}

/// The maps of a stock recipe, baked by the generator directly: what the table
/// must hand to the renderer, made without `load_dir` or `load_recipe`.
fn baked(name: &str) -> mscent_texgen::Maps {
    let path = textures_dir().join(format!("{name}.mst"));
    let recipe = mscent_texgen::recipe::read(&std::fs::read(path).unwrap()).unwrap();
    mscent_texgen::bake(&recipe, recipe.size).unwrap()
}

fn stock_texture<'a>(table: &'a MaterialTable, name: &str) -> &'a Texture {
    &table.get(table.find(name).unwrap()).unwrap().texture
}

/// The four sky materials, which are self-lit cutouts and make no height or normals.
fn is_sky(name: &str) -> bool {
    name.starts_with("sky-")
}

fn solid(name: &str) -> Material {
    Material {
        name: name.to_string(),
        gloss: 7,
        texture: Texture::new(2, 2, vec![0xff11_2233; 4]).unwrap(),
    }
}

#[test]
fn assets_load_and_texels_read_back() {
    let table = MaterialTable::load_dir(&textures_dir()).unwrap();
    let names: Vec<&str> = table.iter().map(|(_, m)| m.name.as_str()).collect();
    assert_eq!(
        names,
        [
            "brick",
            "floor",
            "lightpanel",
            "metal",
            "rock",
            "bark",
            "cap",
            "coral",
            "stem",
            "wax",
            "sky-stars",
            "sky-clouds",
            "sky-ridge",
            "sky-planet"
        ]
    );

    for name in names {
        let (tex, maps) = (stock_texture(&table, name), baked(name));
        let (w, h) = (maps.width, maps.height);
        assert_eq!((tex.width(), tex.height()), (w, h), "{name}");
        // The sky materials are self-lit and have no relief to read back.
        assert_eq!(tex.has_heights(), !is_sky(name), "{name}");
        // Corners, an interior texel and a texel on the other side of a tile.
        for (x, y) in [(0, 0), (5, 7), (w - 1, h - 1), (64, 3), (3, 64)] {
            let i = (y * w + x) as usize;
            let (x, y) = (x as i32, y as i32);
            assert_eq!(tex.texel(x, y), maps.albedo[i], "{name} albedo {x},{y}");
            let height = maps.heights.as_ref().map(|heights| heights[i]);
            assert_eq!(tex.height_at(x, y), height, "{name} height");
        }
    }
    // Cutouts keep their alpha, and the opaque ones keep no plane.
    for (name, masked) in [
        ("sky-stars", false),
        ("sky-clouds", true),
        ("sky-ridge", true),
        ("sky-planet", true),
        ("brick", false),
    ] {
        assert_eq!(stock_texture(&table, name).is_masked(), masked, "{name}");
    }
    // The recipes differ, so reading the wrong file would show.
    assert_ne!(
        stock_texture(&table, "brick").texel(5, 7),
        stock_texture(&table, "metal").texel(5, 7)
    );
}

#[test]
fn normal_maps_decode_to_unit_vectors() {
    let table = MaterialTable::load_dir(&textures_dir()).unwrap();
    for (_, m) in table.iter() {
        let tex = &m.texture;
        if is_sky(&m.name) {
            assert!(!tex.has_normals() && !tex.has_heights(), "{}", m.name);
            continue;
        }
        assert!(tex.has_normals() && tex.has_heights(), "{}", m.name);
        let mut tilted = false;
        for y in 0..128 {
            for x in 0..128 {
                // `normal` normalizes, so the stored texel must already be
                // unit length for that to change nothing.
                let raw = tex.normal_texel(x, y).unwrap();
                let c = |shift: u32| f32::from(((raw >> shift) & 0xff) as u8) / 127.5 - 1.0;
                let stored = Vec3::new(c(16), c(8), c(0));
                assert!((stored.length() - 1.0).abs() < 0.02, "{} {x},{y}", m.name);
                let n = tex.normal(x, y).unwrap();
                assert!(n.z > 0.0, "{} points into the wall at {x},{y}", m.name);
                assert!((n.length() - 1.0).abs() < 1e-4);
                tilted |= n.z < 0.9995;
            }
        }
        // Cap and wax are flat on purpose: bumps read as noise on a curve.
        let smooth = ["cap", "wax"].contains(&m.name.as_str());
        assert!(tilted || smooth, "{} is flat everywhere", m.name);
    }
}

#[test]
fn stock_gloss_comes_from_the_manifest_with_the_old_values() {
    let table = MaterialTable::load_dir(&textures_dir()).unwrap();
    let gloss = |n: &str| table.get(table.find(n).unwrap()).unwrap().gloss;
    assert_eq!(gloss("metal"), 200);
    assert_eq!(gloss("lightpanel"), 120);
    assert_eq!(gloss("floor"), 90);
    assert_eq!(gloss("brick"), 25);
    assert_eq!(gloss("rock"), 10);
    assert_eq!(table.find("nope"), None);
}

#[test]
fn stock_ids_match_the_constants_levels_are_built_with() {
    use mscent_world::materials::{BRICK, FLOOR, LIGHTPANEL, METAL, NAMES, ROCK};
    let table = MaterialTable::load_dir(&textures_dir()).unwrap();
    assert_eq!([BRICK, FLOOR, LIGHTPANEL, METAL, ROCK], [0, 1, 2, 3, 4]);
    for (id, name) in (0u16..).zip(NAMES) {
        assert_eq!(table.find(name), Some(MaterialId(id)), "{name}");
    }
}

#[test]
fn only_metal_and_floor_have_a_mask() {
    let table = MaterialTable::load_dir(&textures_dir()).unwrap();
    for (_, m) in table.iter() {
        let expected = matches!(m.name.as_str(), "metal" | "floor");
        let masked = m.texture.spec_mean() < 1.0;
        if is_sky(&m.name) {
            assert!(!masked, "{} has no mask", m.name);
            continue;
        }
        assert_eq!(
            masked,
            expected,
            "{} (mean {})",
            m.name,
            m.texture.spec_mean()
        );
        // The mask rides in the alpha of the normal texels: an unmasked
        // material has it full everywhere.
        let coords = (0..128).flat_map(|y| (0..128).map(move |x| (x, y)));
        let full = coords
            .map(|(x, y)| m.texture.normal_texel(x, y).unwrap() >> 24)
            .all(|a| a == 255);
        assert_eq!(full, !expected, "{}", m.name);
    }
}

#[test]
fn missing_directory_is_an_error_not_a_panic() {
    let missing = textures_dir().join("does_not_exist");
    assert!(matches!(
        MaterialTable::load_dir(&missing),
        Err(TextureError::Io(_))
    ));
}

#[test]
fn clones_share_the_pixels() {
    let mut table = MaterialTable::new();
    let id = table.add(solid("a")).unwrap();
    let copy = table.clone();
    assert!(std::ptr::eq(table.get(id).unwrap(), copy.get(id).unwrap()));
}

#[test]
fn filtering_a_shared_table_leaves_the_other_holder_alone() {
    let mut table = MaterialTable::new();
    let id = table.add(solid("a")).unwrap();
    let plain = table.clone();
    let filtered = table.filtered();
    assert!(filtered.get(id).unwrap().texture.is_filtered());
    assert!(!plain.get(id).unwrap().texture.is_filtered());
    assert!(!std::ptr::eq(
        plain.get(id).unwrap(),
        filtered.get(id).unwrap()
    ));
}

#[test]
fn filtering_a_filtered_table_copies_nothing() {
    let mut table = MaterialTable::new();
    let id = table.add(solid("a")).unwrap();
    let table = table.filtered();
    let again = table.clone().filtered();
    assert!(std::ptr::eq(table.get(id).unwrap(), again.get(id).unwrap()));
}

#[test]
fn a_retired_id_holds_its_place_and_finds_nothing() {
    let mut table = MaterialTable::new();
    table.add(solid("a")).unwrap();
    let gone = table.add_retired().unwrap();
    let last = table.add(solid("b")).unwrap();
    assert_eq!((gone, last), (MaterialId(1), MaterialId(2)));
    assert_eq!(table.len(), 3);
    assert!(table.get(gone).is_none());
    assert_eq!(table.find("b"), Some(last));
    let ids: Vec<u16> = table.iter().map(|(id, _)| id.0).collect();
    assert_eq!(ids, [0, 2]);
    assert!(table.filtered().get(gone).is_none());
}

/// Saved levels and the voxel generators store these ids, and only the first five
/// are checked when a level is built. The sky materials are held the same way.
#[test]
fn the_living_and_sky_materials_keep_their_ids_in_the_real_manifest() {
    use mscent_world::materials::{
        BARK, CAP, CORAL, SKY_CLOUDS, SKY_PLANET, SKY_RIDGE, SKY_STARS, STEM, WAX,
    };
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/textures");
    let table = MaterialTable::load_dir(&dir).expect("loads");
    for (id, name) in [
        (BARK, "bark"),
        (CAP, "cap"),
        (CORAL, "coral"),
        (STEM, "stem"),
        (WAX, "wax"),
        (SKY_STARS, "sky-stars"),
        (SKY_CLOUDS, "sky-clouds"),
        (SKY_RIDGE, "sky-ridge"),
        (SKY_PLANET, "sky-planet"),
    ] {
        assert_eq!(table.find(name), Some(MaterialId(id)), "{name}");
    }
}

#[test]
fn filtering_a_table_keeps_each_materials_coverage() {
    let cut = Texture::new(2, 2, vec![0xff11_2233; 4])
        .unwrap()
        .with_alpha(vec![0, 0, 255, 255])
        .unwrap();
    let mut table = MaterialTable::new();
    let id = table
        .add(Material {
            name: "cut".into(),
            gloss: 0,
            texture: cut,
        })
        .unwrap();
    let filtered = table.clone().filtered();
    let t = &filtered.get(id).unwrap().texture;
    assert!(t.is_masked());
    assert_eq!(t.alpha_sampler(1).unwrap().nearest(0, 0), 127);
    assert!(!table.get(id).unwrap().texture.is_filtered());
}
