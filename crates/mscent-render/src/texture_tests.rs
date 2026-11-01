use std::path::PathBuf;

use super::*;

fn textures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets/textures")
}

fn rgb(r: u8, g: u8, b: u8) -> u32 {
    u32::from_be_bytes([255, r, g, b])
}

#[test]
fn assets_load_and_texels_read_back() {
    let table = MaterialTable::load_dir(&textures_dir()).unwrap();
    let names: Vec<&str> = table.materials.iter().map(|m| m.name.as_str()).collect();
    assert_eq!(names, ["brick", "floor", "lightpanel", "metal", "rock"]);

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
fn gloss_comes_from_the_material_table() {
    let table = MaterialTable::load_dir(&textures_dir()).unwrap();
    let gloss = |n: &str| table.get(table.find(n).unwrap()).unwrap().gloss;
    assert!(gloss("metal") > gloss("floor"));
    assert!(gloss("floor") > gloss("rock"));
    assert_eq!(table.find("nope"), None);
}

#[test]
fn wrap_addressing_handles_negative_and_large_coordinates() {
    let t = Texture::new(2, 2, vec![1, 2, 3, 4]).unwrap();
    assert_eq!(t.texel(0, 0), 1);
    assert_eq!(t.texel(-1, 0), 2);
    assert_eq!(t.texel(0, -1), 3);
    assert_eq!(t.texel(3, 5), 4);
    assert_eq!(t.texel(i32::MIN, i32::MAX), 3);
}

#[test]
fn construction_validates_sizes() {
    assert!(Texture::new(3, 2, vec![0; 6]).is_err());
    assert!(Texture::new(2, 2, vec![0; 3]).is_err());
    assert!(Texture::new(0, 2, vec![]).is_err());
    let t = Texture::new(2, 2, vec![0; 4]).unwrap();
    assert!(t.clone().with_normals(vec![0; 3]).is_err());
    assert!(t.with_heights(vec![0; 5]).is_err());
}

#[test]
fn missing_directory_is_an_error_not_a_panic() {
    let missing = textures_dir().join("does_not_exist");
    assert!(matches!(
        MaterialTable::load_dir(&missing),
        Err(TextureError::Io(_))
    ));
}
