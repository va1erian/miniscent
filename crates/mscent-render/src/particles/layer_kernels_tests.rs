use super::*;
use mscent_math::rng::Rng;

fn levels() -> impl Iterator<Item = Level> {
    Level::ALL.into_iter().filter(|l| l.is_supported())
}

#[test]
fn every_level_matches_the_scalar_definition_on_random_rows() {
    let mut rng = Rng::new(0xc0de);
    for len in 0..=70usize {
        for _ in 0..12 {
            let dst: Vec<u32> = (0..len).map(|_| rng.next_u64() as u32).collect();
            // Layer lengths from short (leaves the tail alone) to over-long,
            // with untouched pixels among them, singly and in runs.
            let layers = len.div_ceil(2) + rng.below(4) as usize;
            let layer: Vec<u32> = (0..layers)
                .map(|_| {
                    if rng.below(3) == 0 {
                        CLEAR
                    } else {
                        rng.next_u64() as u32
                    }
                })
                .collect();
            let mut want = dst.clone();
            for (i, d) in want.iter_mut().enumerate().take(2 * layer.len()) {
                *d = composite_px(*d, layer[i / 2]);
            }
            for level in levels() {
                let mut got = dst.clone();
                composite_row(level, &mut got, &layer);
                assert_eq!(got, want, "{level:?} len {len}");
            }
        }
    }
}

#[test]
fn a_short_layer_leaves_the_rest_of_the_row_alone() {
    let dst = [0x1234_5678u32; 9];
    for level in levels() {
        let mut got = dst;
        composite_row(level, &mut got, &[0xff00_0001; 2]);
        assert_eq!(got[4..], dst[4..], "{level:?}");
        assert_eq!(
            got[..4],
            [0x1234_5679; 4],
            "{level:?}: alpha kept, colour added"
        );
    }
}

#[test]
fn every_keep_and_colour_matches_on_every_level() {
    // Exhaustive over keep, where a 16-bit overflow would show.
    let layer: Vec<u32> = (0..=255u32).map(|a| a << 24 | 0x00ff_8010).collect();
    for dst0 in [0xffff_ffffu32, 0xff00_0000, 0xff80_ff01] {
        let dst = vec![dst0; 512];
        let mut want = dst.clone();
        composite_scalar(&mut want, &layer);
        for level in levels() {
            let mut got = dst.clone();
            composite_row(level, &mut got, &layer);
            assert_eq!(got, want, "{level:?} {dst0:x}");
        }
    }
}

#[test]
fn the_definition_has_the_documented_rounding_and_identity() {
    // An untouched layer pixel (alpha 255, no colour) changes nothing.
    for p in [0xff00_0000u32, 0xffff_ffff, 0xff12_3456] {
        assert_eq!(composite_px(p, 0xff00_0000), p);
    }
    // Keep 1/256: only the layer's own colour is left; 255 * 1 / 256 = 0.
    assert_eq!(composite_px(0xffff_ffff, 0x0010_2030), 0xff10_2030);
    // The frame's alpha byte is not touched.
    assert_eq!(composite_px(0x40ff_ffff, 0x0010_2030), 0x4010_2030);
    // Half kept (alpha 127): 200 * 128 / 256 = 100, plus 10.
    assert_eq!(composite_px(0xffc8_c8c8, 0x7f0a_0a0a), 0xff6e_6e6e);
    // Saturates.
    assert_eq!(composite_px(0xffff_ffff, 0xfff0_f0f0), 0xffff_ffff);
}

#[test]
fn an_untouched_layer_pixel_leaves_the_frame_pixel_as_it_was() {
    let dst = [0x1234_5678u32; 16];
    for level in levels() {
        let mut got = dst;
        composite_row(level, &mut got, &[CLEAR; 8]);
        assert_eq!(got, dst, "{level:?}");
    }
}

fn depth_row(rng: &mut Rng, n: usize) -> Vec<f32> {
    (0..n)
        .map(|_| match rng.below(10) {
            0 => 0.0,
            1 => -0.0,
            2 => f32::NAN,
            _ => rng.range_f32(0.0, 1.0),
        })
        .collect()
}

#[test]
fn nearest_blocks_match_the_scalar_definition_on_every_level_bit_for_bit() {
    let mut rng = Rng::new(0xdee9);
    for len in 0..=70usize {
        for _ in 0..12 {
            let (top, bottom) = (depth_row(&mut rng, len), depth_row(&mut rng, len));
            let blocks = len.div_ceil(2);
            let mut want = vec![0.0; blocks];
            nearest_scalar(&mut want, &top, &bottom);
            for level in levels() {
                let mut got = vec![0.0; blocks];
                nearest_row(level, &mut got, &top, &bottom);
                let bits = |v: &[f32]| v.iter().map(|f| f.to_bits()).collect::<Vec<_>>();
                assert_eq!(bits(&got), bits(&want), "{level:?} len {len}");
            }
        }
    }
}

#[test]
fn nearest_takes_the_largest_of_each_block_and_a_block_of_one_at_an_odd_tail() {
    let top = [0.1, 0.5, 0.2, 0.2, 0.9];
    let bottom = [0.3, 0.1, 0.4, 0.0, 0.2];
    for level in levels() {
        let mut out = [0.0; 3];
        nearest_row(level, &mut out, &top, &bottom);
        assert_eq!(out, [0.5, 0.4, 0.9], "{level:?}");
    }
}
