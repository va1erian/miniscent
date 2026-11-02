use super::*;
use mscent_math::rng::Rng;

const BLENDS: [Blend; 2] = [Blend::Add, Blend::Over];

fn row(rng: &mut Rng, n: usize) -> (Vec<u32>, Vec<f32>, Vec<u8>) {
    let dst = (0..n).map(|_| rng.next_u64() as u32).collect();
    // Depth values straddle the sprite's, including equal, zero and NaN.
    let depth = (0..n)
        .map(|_| match rng.below(8) {
            0 => 0.5,
            1 => 0.0,
            2 => f32::NAN,
            _ => rng.range_f32(0.0, 1.0),
        })
        .collect();
    let mask = (0..n).map(|_| rng.next_u64() as u8).collect();
    (dst, depth, mask)
}

fn per_pixel(blend: Blend, dst: &[u32], depth: &[f32], mask: &[u8], tint: u32, z: f32) -> Vec<u32> {
    per_pixel_in(blend, Dest::Frame, dst, depth, mask, tint, z)
}

fn per_pixel_in(
    blend: Blend,
    dest: Dest,
    dst: &[u32],
    depth: &[f32],
    mask: &[u8],
    tint: u32,
    z: f32,
) -> Vec<u32> {
    let f = match (blend, dest) {
        (Blend::Add, Dest::Frame) => add_px::<false>,
        (Blend::Over, Dest::Frame) => over_px::<false>,
        (Blend::Add, Dest::Layer) => add_px::<true>,
        (Blend::Over, Dest::Layer) => over_px::<true>,
    };
    (0..dst.len())
        .map(|i| {
            if z >= depth[i] {
                f(dst[i], u32::from(mask[i]), tint)
            } else {
                dst[i]
            }
        })
        .collect()
}

#[test]
fn every_level_matches_the_scalar_definition_on_random_rows() {
    let mut rng = Rng::new(0x5eed);
    for len in 0..=70 {
        for _ in 0..12 {
            let (dst, depth, mask) = row(&mut rng, len);
            // Arbitrary tints, including ones that are not premultiplied.
            let tint = rng.next_u64() as u32;
            let z = [0.5, 0.25, 0.75, 1.0][rng.below(4) as usize];
            for blend in BLENDS {
                let want = per_pixel(blend, &dst, &depth, &mask, tint, z);
                for level in Level::ALL.into_iter().filter(|l| l.is_supported()) {
                    let mut got = dst.clone();
                    blend_row(
                        level,
                        (blend, Dest::Frame),
                        &mut got,
                        &depth,
                        &mask,
                        tint,
                        z,
                    );
                    assert_eq!(got, want, "{level:?} {blend:?} len {len}");
                }
            }
        }
    }
}

#[test]
fn every_coverage_and_tint_alpha_matches_on_every_level() {
    // Exhaustive over coverage and alpha, where a 16-bit overflow would show.
    let mask: Vec<u8> = (0..=255).collect();
    let depth = vec![0.0f32; 256];
    for alpha in (0..=255u32).step_by(5).chain([255]) {
        let tint = alpha << 24 | 0x00ff_80ff;
        for blend in BLENDS {
            for dst0 in [0xffff_ffffu32, 0xff00_0000, 0x00ff_8040] {
                let dst = vec![dst0; 256];
                let want = per_pixel(blend, &dst, &depth, &mask, tint, 1.0);
                for level in Level::ALL.into_iter().filter(|l| l.is_supported()) {
                    let mut got = dst.clone();
                    blend_row(
                        level,
                        (blend, Dest::Frame),
                        &mut got,
                        &depth,
                        &mask,
                        tint,
                        1.0,
                    );
                    assert_eq!(got, want, "{level:?} {blend:?} a {alpha}");
                }
            }
        }
    }
}

#[test]
fn definitions_have_the_documented_rounding() {
    // 128 * 255 / 256 = 127 (rounds down); 100 + 127 = 227.
    assert_eq!(add_px::<false>(0xff00_0064, 128, 0xff00_00ff), 0xff00_00e3);
    // Saturates at 255.
    assert_eq!(add_px::<false>(0xff00_00f0, 255, 0xff00_00ff), 0xff00_00ff);
    // Full coverage, opaque tint replaces the pixel (127 for tint 128, as the destination is scaled to 2/256).
    assert_eq!(over_px::<false>(0xff12_3456, 255, 0xff80_8080), 0xff7f_7f7f);
    // Zero coverage keeps the colour and still sets alpha.
    assert_eq!(over_px::<false>(0x0012_3456, 0, 0xff80_8080), 0xff12_3456);
    // s = 128 * 128 / 256 = 64; keep = 256 - 64 = 192; 100 * 192 / 256 = 75.
    assert_eq!(over_px::<false>(0xff00_0064, 128, 0x8000_0080) & 0xff, 139);
}

#[test]
fn failed_depth_test_leaves_pixels_untouched_even_their_alpha() {
    let dst = [0x1234_5678u32; 9];
    let depth = [2.0f32; 9];
    let mask = [255u8; 9];
    for blend in BLENDS {
        for level in Level::ALL.into_iter().filter(|l| l.is_supported()) {
            let mut got = dst;
            blend_row(
                level,
                (blend, Dest::Frame),
                &mut got,
                &depth,
                &mask,
                0xffff_ffff,
                1.0,
            );
            assert_eq!(got, dst, "{level:?} {blend:?}");
        }
    }
}

#[test]
fn mismatched_lengths_use_the_shortest() {
    let mut dst = [0xff00_0000u32; 6];
    blend_row(
        Level::best(),
        (Blend::Add, Dest::Frame),
        &mut dst,
        &[0.0; 4],
        &[255; 5],
        0xffff_ffff,
        1.0,
    );
    assert!(dst[..4].iter().all(|&p| p != 0xff00_0000));
    assert!(dst[4..].iter().all(|&p| p == 0xff00_0000));
}

#[test]
fn every_level_matches_the_scalar_definition_in_the_layer() {
    let mut rng = Rng::new(0x1a7e);
    for len in 0..=70 {
        for _ in 0..12 {
            let (dst, depth, mask) = row(&mut rng, len);
            let tint = rng.next_u64() as u32;
            let z = [0.5, 0.25, 0.75, 1.0][rng.below(4) as usize];
            for blend in BLENDS {
                let want = per_pixel_in(blend, Dest::Layer, &dst, &depth, &mask, tint, z);
                for level in Level::ALL.into_iter().filter(|l| l.is_supported()) {
                    let mut got = dst.clone();
                    blend_row(
                        level,
                        (blend, Dest::Layer),
                        &mut got,
                        &depth,
                        &mask,
                        tint,
                        z,
                    );
                    assert_eq!(got, want, "{level:?} {blend:?} len {len}");
                }
            }
        }
    }
}

#[test]
fn every_coverage_and_tint_alpha_matches_in_the_layer() {
    let mask: Vec<u8> = (0..=255).collect();
    let depth = vec![0.0f32; 256];
    for alpha in (0..=255u32).step_by(5).chain([255]) {
        let tint = alpha << 24 | 0x00ff_80ff;
        for blend in BLENDS {
            for dst0 in [0xffff_ffffu32, 0xff00_0000, 0x7fff_8040, 0x0000_0000] {
                let dst = vec![dst0; 256];
                let want = per_pixel_in(blend, Dest::Layer, &dst, &depth, &mask, tint, 1.0);
                for level in Level::ALL.into_iter().filter(|l| l.is_supported()) {
                    let mut got = dst.clone();
                    blend_row(
                        level,
                        (blend, Dest::Layer),
                        &mut got,
                        &depth,
                        &mask,
                        tint,
                        1.0,
                    );
                    assert_eq!(got, want, "{level:?} {blend:?} a {alpha}");
                }
            }
        }
    }
}

#[test]
fn the_layer_keeps_alpha_as_a_product_of_keep_factors() {
    // Keep 256 - 128 * 128 / 256 = 192 per sprite: 255 -> 191 -> 143.
    let tint = 0x8000_0080;
    let once = over_px::<true>(0xff00_0000, 128, tint);
    assert_eq!(once >> 24, 191);
    assert_eq!(over_px::<true>(once, 128, tint) >> 24, 143);
    // No coverage keeps everything: 255 * 256 / 256.
    assert_eq!(over_px::<true>(0xff00_0000, 0, tint) >> 24, 255);
    // "Add" never touches it, and neither does a failed depth test.
    assert_eq!(add_px::<true>(0x4000_0000, 255, 0xffff_ffff) >> 24, 0x40);
    // The sprite's own alpha is not stored: colour accumulates alone.
    assert_eq!(
        over_px::<true>(0xff00_0000, 255, 0xff00_0010) & 0x00ff_ffff,
        0x10 * 255 / 256
    );
}
