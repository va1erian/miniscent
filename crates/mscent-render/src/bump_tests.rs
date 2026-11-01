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
