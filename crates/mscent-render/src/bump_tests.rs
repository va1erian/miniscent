use super::*;

#[test]
fn decode_maps_bytes_to_unit_range() {
    let flat = decode_normal(0xff80_80ff);
    assert!(flat.x.abs() < 0.01 && flat.y.abs() < 0.01 && (flat.z - 1.0).abs() < 1e-6);
    let side = decode_normal(0xffff_8080);
    assert!((side.x - 1.0).abs() < 1e-6);
}

#[test]
fn flat_normal_gain_is_one() {
    let l = Vec3::new(0.3, 0.2, 0.9).normalized().unwrap();
    assert!((bump_gain(FLAT_NORMAL, l) - 1.0).abs() < 1e-6);
}

#[test]
fn tilting_toward_light_brightens_and_away_darkens() {
    let l = Vec3::new(0.6, 0.0, 0.8);
    let toward = Vec3::new(0.3, 0.0, 0.95).normalized().unwrap();
    let away = Vec3::new(-0.3, 0.0, 0.95).normalized().unwrap();
    assert!(bump_gain(toward, l) > 1.0);
    assert!(bump_gain(away, l) < 1.0);
}

#[test]
fn gain_is_bounded_and_nonnegative() {
    let grazing = Vec3::new(1.0, 0.0, 0.0);
    assert_eq!(bump_gain(Vec3::new(-1.0, 0.0, 0.0), grazing), MIN_GAIN);
    assert!(bump_gain(grazing, grazing) <= MAX_GAIN);
}

#[test]
fn magnified_normals_interpolate_between_texel_centres() {
    // 2x2 map: left column tilts -x, right column tilts +x.
    let (l, r) = (0xff00_80c0u32, 0xffff_80c0u32);
    let tex = crate::texture::Texture::new(2, 2, vec![0xff00_0000; 4])
        .unwrap()
        .with_normals(vec![l, r, l, r])
        .unwrap();
    let at = |fu: i32| texel_normal(&tex, fu, 0x8000).x;
    // Texel centres sit at half-integers: 0.5 is the left texel, 1.5 the right.
    assert!(at(0x8000) < -0.9 && at(0x18000) > 0.9);
    let (a, b, c) = (at(0xc000), at(0x10000), at(0x14000));
    assert!(a < b && b < c, "{a} {b} {c}");
    assert!(b.abs() < 0.05);
}

#[test]
fn flat_normal_gain_is_one_even_for_grazing_light() {
    let grazing = Vec3::new(0.99, 0.0, 0.1).normalized().unwrap();
    assert!((bump_gain(FLAT_NORMAL, grazing) - 1.0).abs() < 1e-6);
}
