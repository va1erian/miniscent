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
    // Levels store these ids: the first five must never move, whatever is added.
    assert_eq!(
        names[..5],
        ["brick", "floor", "lightpanel", "metal", "rock"]
    );
    assert_eq!(names, mscent_world::materials::NAMES);

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

fn stripes() -> Texture {
    // 4x4 vertical stripes, one texel wide, black and white.
    let px: Vec<u32> = (0..16)
        .map(|i| if i % 2 == 0 { 0xff00_0000 } else { 0xffff_ffff })
        .collect();
    Texture::new(4, 4, px).unwrap()
}

#[test]
fn plain_textures_are_nearest_and_filtered_ones_have_mips() {
    let t = stripes();
    assert!(!t.is_filtered() && t.mip_count() == 0);
    let f = t.clone().filtered();
    assert!(f.is_filtered());
    assert_eq!(f.mip_count(), 2);
    // The stripes average to mid grey one level down.
    let g = f.texel_lod(0, 0, 1);
    assert!((0x70..=0x8f).contains(&(g & 0xff)), "{g:#x}");
    assert_eq!(f.texel_lod(0, 0, 0), 0xff00_0000);
    // Levels beyond the chain clamp to the last one.
    assert_eq!(f.texel_lod(1, 3, 9), f.texel_lod(0, 0, 2));
}

#[test]
fn bilinear_returns_texel_centres_and_blends_between_them() {
    let f = stripes().filtered();
    let centre = |t: i32| (t << 16) + 0x8000;
    assert_eq!(f.sample_bilinear(centre(0), centre(0)), 0xff00_0000);
    assert_eq!(f.sample_bilinear(centre(1), centre(0)), 0xffff_ffff);
    let mid = f.sample_bilinear(1 << 16, centre(0));
    assert!((0x78..=0x88).contains(&(mid & 0xff)), "{mid:#x}");
    // Wraps around the edge instead of reading out of bounds.
    let _ = f.sample_bilinear(-1, -1);
    let _ = f.sample_bilinear(i32::MAX, i32::MIN);
}
