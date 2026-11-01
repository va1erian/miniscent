use super::*;

#[test]
fn matte_gloss_has_no_highlight() {
    let t = SpecTable::new(0);
    assert_eq!(t.response(1.0), 0.0);
}

#[test]
fn response_peaks_at_one_and_falls_off() {
    let t = SpecTable::new(200);
    assert!((t.response(1.0) - 200.0 / 255.0).abs() < 1e-6);
    assert!(t.response(0.99) > t.response(0.9));
    assert!(t.response(0.9) > t.response(0.5));
    assert_eq!(t.response(-0.5), 0.0);
}

#[test]
fn higher_gloss_is_tighter() {
    let (lo, hi) = (SpecTable::new(40), SpecTable::new(240));
    // Relative falloff away from the peak.
    assert!(hi.response(0.9) / hi.response(1.0) < lo.response(0.9) / lo.response(1.0));
}

#[test]
fn half_vector_is_unit_and_bisects() {
    let l = Vec3::new(1.0, 0.0, 0.0);
    let v = Vec3::new(0.0, 0.0, 1.0);
    let h = half_vector(l, v);
    assert!((h.length() - 1.0).abs() < 1e-6);
    assert!((h.x - h.z).abs() < 1e-6);
    assert_eq!(half_vector(l, -l), l);
}

#[test]
fn response_is_interpolated_between_table_entries() {
    let t = SpecTable::new(200);
    let (a, b) = (0.9000_f32, 0.9020_f32);
    let (ra, rm, rb) = (t.response(a), t.response((a + b) / 2.0), t.response(b));
    // Strictly between the ends, not equal to one of them.
    assert!(rm > ra.min(rb) && rm < ra.max(rb));
}

#[test]
fn matte_materials_are_not_drawn_with_highlights() {
    assert!(!SpecTable::new(0).is_visible());
    assert!(!SpecTable::new(10).is_visible());
    assert!(SpecTable::new(40).is_visible());
    assert!(SpecTable::new(200).is_visible());
}
