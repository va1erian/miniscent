use super::*;

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

#[test]
fn filtering_twice_keeps_the_same_chain() {
    let once = stripes().filtered();
    let twice = once.clone().filtered();
    assert_eq!(once.mip_count(), twice.mip_count());
    assert_eq!(once.texel_lod(1, 1, 1), twice.texel_lod(1, 1, 1));
}

#[test]
fn a_mask_needs_one_byte_per_texel_and_defaults_to_full() {
    let t = Texture::new(2, 2, vec![0; 4]).unwrap();
    assert_eq!(t.spec_mean(), 1.0, "no mask at all is exactly full");
    assert!(matches!(
        t.clone().with_specular(&[0; 3]),
        Err(TextureError::SizeMismatch)
    ));
    assert!(matches!(
        t.clone().with_specular(&[0; 5]),
        Err(TextureError::SizeMismatch)
    ));
    let full = t.clone().with_normals(vec![0xff80_80ff; 4]).unwrap();
    assert_eq!(full.spec_mean(), 1.0);
}

#[test]
fn a_mask_goes_into_the_normal_alpha_and_leaves_the_normal_alone() {
    let normals = vec![0xff12_3456, 0xff65_4321, 0xff00_0000, 0xffff_ffff];
    let t = Texture::new(2, 2, vec![0; 4])
        .unwrap()
        .with_normals(normals.clone())
        .unwrap()
        .with_specular(&[0, 100, 200, 255])
        .unwrap();
    assert_eq!(t.normal_texel(0, 0), Some(0x0012_3456));
    assert_eq!(t.normal_texel(1, 0), Some(0x6465_4321));
    assert_eq!(t.normal_texel(0, 1), Some(0xc800_0000));
    assert_eq!(t.normal_texel(1, 1), Some(0xffff_ffff));
    // (0 + 100 + 200 + 255) / 4 = 138.75, rounded down so that only an all-full
    // mask has a mean of 255.
    assert_eq!(t.spec_mean(), 138.0 / 255.0);
    let nearly = Texture::new(16, 16, vec![0; 256]).unwrap();
    let mut mask = [255; 256];
    mask[7] = 0;
    assert!(nearly.with_specular(&mask).unwrap().spec_mean() < 1.0);
    let plain = Texture::new(2, 2, vec![0; 4]).unwrap();
    let plain = plain.with_normals(normals).unwrap();
    for (x, y) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
        assert_eq!(
            t.normal(x, y),
            plain.normal(x, y),
            "alpha is not the normal"
        );
    }
}

#[test]
fn a_mask_with_no_normal_map_makes_a_flat_one() {
    let t = Texture::new(2, 1, vec![0; 2])
        .unwrap()
        .with_specular(&[10, 20])
        .unwrap();
    assert!(t.has_normals());
    assert_eq!(t.normal_texel(0, 0), Some(0x0a80_80ff));
    assert_eq!(t.normal_texel(1, 0), Some(0x1480_80ff));
}
