use super::*;

#[test]
fn the_image_fills_the_window_at_its_aspect_ratio() {
    assert_eq!(fit_rect(4, 3, 14, 10), (0, 0, 13, 10));
    assert_eq!(fit_rect(4, 3, 4, 3), (0, 0, 4, 3));
    // 16:9 into 4:3 gets bars above and below.
    assert_eq!(fit_rect(16, 9, 1280, 960), (0, 120, 1280, 720));
    // A tall window gets bars at the sides of a narrower image.
    assert_eq!(fit_rect(4, 4, 6, 4), (1, 0, 4, 4));
}

#[test]
fn smaller_window_shrinks_keeping_aspect() {
    let (x, y, w, h) = fit_rect(8, 4, 4, 4);
    assert_eq!((w, h), (4, 2));
    assert_eq!((x, y), (0, 1));
    assert_eq!(fit_rect(0, 4, 4, 4), (0, 0, 0, 0));
}

#[test]
fn whole_factors_replicate_pixels_and_black_the_bars() {
    let src = [1, 2, 3, 4];
    let mut dst = [9u32; 5 * 4];
    blit_fit(&src, 2, 2, &mut dst, 5, 4);
    // 2x in a 5x4 window: a 4x4 image at x = 0 (offset (5-4)/2 = 0).
    assert_eq!(&dst[0..5], &[1, 1, 2, 2, 0]);
    assert_eq!(&dst[5..10], &[1, 1, 2, 2, 0]);
    assert_eq!(&dst[10..15], &[3, 3, 4, 4, 0]);
    assert_eq!(&dst[15..20], &[3, 3, 4, 4, 0]);
}

#[test]
fn bars_above_and_below_are_cleared() {
    let src = [0xffff_ffff; 4];
    let mut dst = [9u32; 4 * 8];
    blit_fit(&src, 2, 2, &mut dst, 4, 8);
    assert!(dst[..8].iter().all(|&p| p == 0), "top bar");
    assert!(dst[24..].iter().all(|&p| p == 0), "bottom bar");
    assert!(dst[8..24].iter().all(|&p| p == 0xffff_ffff), "image");
}

#[test]
fn fractional_magnification_blends_neighbours_smoothly() {
    // Black to white across two pixels, shown 2.5 times larger: each row
    // rises monotonically from black to white with a mid grey in between.
    let (b, w) = (0xff00_0000, 0xffff_ffff);
    let mut dst = [7u32; 25];
    blit_fit(&[b, w, b, w], 2, 2, &mut dst, 5, 5);
    for row in dst.chunks(5) {
        let red: Vec<u32> = row.iter().map(|p| (p >> 16) & 0xff).collect();
        assert_eq!((red[0], red[4]), (0, 255), "{red:?}");
        assert!(
            red.windows(2).all(|p| p[0] <= p[1]),
            "not monotonic: {red:?}"
        );
        assert!((120..=135).contains(&red[2]), "middle is {}", red[2]);
        // Every channel is blended alike: still grey.
        assert!(row.iter().all(|p| (p & 0xff) == ((p >> 8) & 0xff)));
    }
}

#[test]
fn rows_blend_too_and_a_flat_image_stays_flat() {
    let (b, w) = (0xff00_0000, 0xffff_ffff);
    let mut dst = [0u32; 25];
    blit_fit(&[b, b, w, w], 2, 2, &mut dst, 5, 5);
    let col: Vec<u32> = (0..5).map(|y| dst[y * 5] & 0xff).collect();
    assert_eq!((col[0], col[4]), (0, 255), "{col:?}");
    assert!(col.windows(2).all(|p| p[0] <= p[1]), "{col:?}");
    let flat = [0xff33_6699; 9];
    let mut out = [0u32; 7 * 5];
    blit_fit(&flat, 3, 3, &mut out, 7, 5);
    // 5x5 image centred in 7x5: every image pixel keeps the colour exactly.
    assert!(
        out.chunks(7)
            .all(|r| r[1..6].iter().all(|&p| p == 0xff33_6699))
    );
    assert!(out.chunks(7).all(|r| r[0] == 0 && r[6] == 0), "side bars");
}

#[test]
fn mismatched_slices_are_ignored() {
    let mut dst = [7u32; 4];
    blit_fit(&[1, 2, 3], 2, 2, &mut dst, 2, 2);
    assert_eq!(dst, [7; 4]);
}
