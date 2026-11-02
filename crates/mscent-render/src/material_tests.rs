use std::path::PathBuf;

use super::*;
use crate::texture::TextureError;

fn textures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets/textures")
}

fn rgb(r: u8, g: u8, b: u8) -> u32 {
    u32::from_be_bytes([255, r, g, b])
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
            "wax"
        ]
    );

    // Expected values were read with an independent decoder (PIL).
    let brick = &table.get(table.find("brick").unwrap()).unwrap().texture;
    assert_eq!((brick.width(), brick.height()), (128, 128));
    assert_eq!(brick.texel(0, 0), rgb(115, 110, 102));
    assert_eq!(brick.texel(5, 7), rgb(186, 74, 49));
    assert_eq!(brick.texel(127, 127), rgb(127, 51, 34));
    assert_eq!(brick.texel(64, 3), rgb(116, 111, 103));
    assert_eq!(brick.height_at(0, 0), Some(40));
    assert_eq!(brick.height_at(5, 7), Some(216));
    assert_eq!(brick.height_at(127, 127), Some(159));

    let metal = &table.get(table.find("metal").unwrap()).unwrap().texture;
    assert_eq!(metal.texel(5, 7), rgb(137, 145, 156));
}

#[test]
fn normal_maps_decode_to_unit_vectors() {
    let table = MaterialTable::load_dir(&textures_dir()).unwrap();
    let brick = &table.get(table.find("brick").unwrap()).unwrap().texture;
    assert!(brick.has_normals() && brick.has_heights());
    // Raw (125,127,255) is a nearly flat normal pointing out of the surface.
    let n = brick.normal(5, 7).unwrap();
    assert!(n.z > 0.99 && n.x.abs() < 0.02 && (n.length() - 1.0).abs() < 1e-4);
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
fn no_stock_material_has_a_mask() {
    let table = MaterialTable::load_dir(&textures_dir()).unwrap();
    for (_, m) in table.iter() {
        assert_eq!(m.texture.spec_mean(), 1.0, "{}", m.name);
        for y in 0..m.texture.height() as i32 {
            for x in 0..m.texture.width() as i32 {
                let texel = m.texture.normal_texel(x, y).unwrap();
                assert_eq!(texel >> 24, 255, "{} at {x},{y}", m.name);
            }
        }
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
/// are checked when a level is built.
#[test]
fn the_living_materials_keep_their_ids_in_the_real_manifest() {
    use mscent_world::materials::{BARK, CAP, CORAL, STEM, WAX};
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/textures");
    let table = MaterialTable::load_dir(&dir).expect("loads");
    for (id, name) in [
        (BARK, "bark"),
        (CAP, "cap"),
        (CORAL, "coral"),
        (STEM, "stem"),
        (WAX, "wax"),
    ] {
        assert_eq!(table.find(name), Some(MaterialId(id)), "{name}");
    }
}
