use super::*;

#[test]
fn an_all_solid_plane_is_no_plane() {
    assert!(AlphaPlane::new(vec![255; 16]).is_none());
    assert!(AlphaPlane::new(vec![255, 255, 255, 254]).is_some());
}

#[test]
fn the_chain_averages_down_to_one_texel() {
    let plane = AlphaPlane::new(vec![0, 255, 255, 0])
        .unwrap()
        .with_mips(2, 2);
    assert_eq!(plane.mips, vec![vec![127]]);
    let rect = AlphaPlane::new(vec![0, 0, 255, 255])
        .unwrap()
        .with_mips(4, 1);
    assert_eq!(rect.mips.len(), 2);
    assert_eq!(rect.mips[0], vec![0, 255]);
    assert_eq!(rect.mips[1], vec![127]);
}

#[test]
fn a_chain_that_exists_is_kept() {
    let plane = AlphaPlane::new(vec![0, 255, 255, 0])
        .unwrap()
        .with_mips(2, 2);
    let again = plane.clone().with_mips(2, 2);
    assert_eq!(plane.mips, again.mips);
}

#[test]
fn a_texel_centre_reads_that_texel_and_the_middle_blends() {
    let plane = AlphaPlane::new(vec![0, 200, 0, 200]).unwrap();
    let s = plane.sampler(2, 2, 0);
    // Centres sit at half-integers in 16.16.
    assert_eq!(s.bilinear(0x8000, 0x8000), 0);
    assert_eq!(s.bilinear(0x1_8000, 0x8000), 200);
    assert_eq!(s.bilinear(0x1_0000, 0x8000), 100);
    assert_eq!(s.nearest(1, 0), 200);
    assert_eq!(s.nearest(3, 2), 200, "wraps");
}

#[test]
fn a_level_past_the_chain_clamps() {
    let plane = AlphaPlane::new(vec![0, 255, 255, 0])
        .unwrap()
        .with_mips(2, 2);
    assert_eq!(plane.sampler(2, 2, 9).nearest(0, 0), 127);
    assert_eq!(plane.sampler(2, 2, 0).nearest(0, 0), 0);
}

#[test]
fn a_short_plane_reads_clear_and_does_not_panic() {
    let plane = AlphaPlane::new(vec![0]).unwrap();
    let s = plane.sampler(4, 4, 0);
    assert_eq!(s.nearest(3, 3), 0);
    assert_eq!(s.bilinear(0x3_8000, 0x3_8000), 0);
}
