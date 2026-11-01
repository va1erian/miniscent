use super::*;

fn end(depth: f32) -> SpanEnd {
    SpanEnd {
        x: 0,
        y: 0,
        u: 0.0,
        v: 0.0,
        depth,
    }
}

#[test]
fn mul_rgb_scales_and_saturates() {
    assert_eq!(mul_rgb(0xff80_4020, 256), 0xff80_4020);
    assert_eq!(mul_rgb(0xff80_4020, 128), 0xff40_2010);
    assert_eq!(mul_rgb(0xff80_4020, 0), 0xff00_0000);
    assert_eq!(mul_rgb(0xffc0_c0c0, 512), 0xffff_ffff);
    assert_eq!(
        mul_rgb(0x0000_0000, 256),
        0xff00_0000,
        "alpha is forced opaque"
    );
}

#[test]
fn lerp_rgb_hits_both_endpoints() {
    assert_eq!(lerp_rgb(0xff10_2030, 0xffa0_b0c0, 0), 0xff10_2030);
    assert_eq!(lerp_rgb(0xff10_2030, 0xffa0_b0c0, 256), 0xffa0_b0c0);
    assert_eq!(lerp_rgb(0xff00_0000, 0xffff_ffff, 9999), 0xffff_ffff);
    assert_eq!(lerp_rgb(0xff00_0000, 0xff80_8080, 128), 0xff40_4040);
}

#[test]
fn fog_amount_ramps_between_start_and_end() {
    let fog = Fog::new(0xff00_0000, 10.0, 20.0).unwrap();
    assert_eq!(fog.amount(5.0), 0.0);
    assert_eq!(fog.amount(15.0), 128.0);
    assert_eq!(fog.amount(99.0), 256.0);
    assert!(Fog::new(0, 5.0, 5.0).is_none());
    assert!(Fog::new(0, -1.0, 5.0).is_none());
    assert!(Fog::new(0, 0.0, f32::NAN).is_none());
}

#[test]
fn flat_shader_without_fog_only_lights() {
    let mut s = FlatShader::new(128, None);
    s.begin_span(&end(1.0), &end(2.0), 16);
    assert_eq!(s.shade(7, 0xff80_8080, 0, 0), 0xff40_4040);
}

#[test]
fn flat_shader_steps_fog_across_the_span() {
    let fog = Fog::new(0xffff_ffff, 0.0, 100.0).unwrap();
    let mut s = FlatShader::new(256, Some(fog));
    s.begin_span(&end(0.0), &end(100.0), 16);
    assert_eq!(
        s.shade(0, 0xff00_0000, 0, 0),
        0xff00_0000,
        "clear at the near end"
    );
    let mid = s.shade(8, 0xff00_0000, 0, 0) & 0xff;
    assert!((120..=136).contains(&mid), "half fogged, got {mid}");
    let last = s.shade(16, 0xff00_0000, 0, 0) & 0xff;
    assert_eq!(last, 255, "fully fogged at the far end");
}
