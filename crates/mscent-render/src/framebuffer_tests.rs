use super::*;

#[test]
fn new_validates_size_and_starts_black_and_empty() {
    assert!(Framebuffer::new(0, 4).is_none());
    assert!(Framebuffer::new(4, 9000).is_none());
    let fb = Framebuffer::new(4, 3).unwrap();
    assert_eq!(fb.pixels().len(), 12);
    assert_eq!(fb.pixel(3, 2), Some(0xff00_0000));
    assert_eq!(fb.depth_at(0, 0), Some(0.0));
}

#[test]
fn out_of_range_reads_are_none() {
    let fb = Framebuffer::new(4, 3).unwrap();
    assert_eq!(fb.pixel(-1, 0), None);
    assert_eq!(fb.pixel(4, 0), None);
    assert_eq!(fb.pixel(0, 3), None);
    assert_eq!(fb.depth_at(0, -1), None);
}

#[test]
fn clear_sets_colour_and_resets_depth() {
    let mut fb = Framebuffer::new(2, 2).unwrap();
    fb.row_mut(0).unwrap().1[0] = 5.0;
    fb.clear(0xff12_3456);
    assert!(fb.pixels().iter().all(|&p| p == 0xff12_3456));
    assert_eq!(fb.depth_at(0, 0), Some(0.0));
}

#[test]
fn sky_gradient_runs_from_top_to_bottom_colour() {
    let mut fb = Framebuffer::new(2, 5).unwrap();
    fb.clear_sky(0xff00_0000, 0xffff_ffff);
    assert_eq!(fb.pixel(0, 0), Some(0xff00_0000));
    assert_eq!(fb.pixel(1, 4), Some(0xffff_ffff));
    let mid = fb.pixel(0, 2).unwrap() & 0xff;
    assert!((120..=136).contains(&mid), "mid grey, got {mid}");
}

#[test]
fn row_mut_rejects_bad_rows() {
    let mut fb = Framebuffer::new(2, 2).unwrap();
    assert!(fb.row_mut(-1).is_none());
    assert!(fb.row_mut(2).is_none());
    assert!(fb.row_mut(1).is_some());
}
