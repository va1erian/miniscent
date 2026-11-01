use super::*;
use mscent_math::rng::Rng;

fn random_pixels(rng: &mut Rng, n: usize) -> Vec<u32> {
    (0..n).map(|_| rng.next_u64() as u32).collect()
}

#[test]
fn best_level_is_supported_and_scalar_always_is() {
    assert!(Level::Scalar.is_supported());
    assert!(Level::best().is_supported());
}

#[test]
fn scale_matches_scalar_on_every_level_and_length() {
    let mut rng = Rng::new(7);
    // Lengths straddle the 4 and 8 pixel vector widths, including empty.
    for len in [0, 1, 3, 4, 5, 7, 8, 9, 15, 16, 17, 100, 641] {
        let src = random_pixels(&mut rng, len);
        for k in [0, 1, 100, 255, 256, 257, 258, 1000, u32::MAX] {
            let mut want = src.clone();
            scale_scalar(&mut want, k);
            for level in Level::ALL {
                let mut got = src.clone();
                scale_rgb_with(level, &mut got, k);
                assert_eq!(got, want, "level {level:?} len {len} k {k}");
            }
        }
    }
}

#[test]
fn lerp_matches_scalar_on_every_level_and_length() {
    let mut rng = Rng::new(11);
    for len in [0, 1, 3, 4, 5, 8, 9, 31, 64, 333] {
        let src = random_pixels(&mut rng, len);
        for t in [0, 1, 64, 127, 128, 255, 256, 300, u32::MAX] {
            let color = rng.next_u64() as u32;
            let mut want = src.clone();
            lerp_scalar(&mut want, color, t.min(256));
            for level in Level::ALL {
                let mut got = src.clone();
                lerp_toward_with(level, &mut got, color, t);
                assert_eq!(got, want, "level {level:?} len {len} t {t}");
            }
        }
    }
}

#[test]
fn every_channel_value_matches_for_all_levels() {
    // Exhaustive over one channel value and every t, which is where a 16-bit
    // overflow would show up.
    for t in 0..=256 {
        let src: Vec<u32> = (0..=255u32).map(|v| v | v << 8 | v << 16).collect();
        let mut want = src.clone();
        lerp_scalar(&mut want, 0x00ff_00ff, t);
        for level in Level::ALL {
            let mut got = src.clone();
            lerp_toward_with(level, &mut got, 0x00ff_00ff, t);
            assert_eq!(got, want, "level {level:?} t {t}");
        }
    }
}

#[test]
fn kernels_agree_with_the_per_pixel_definitions() {
    let mut px = [
        0x8010_20f0u32,
        0x00ff_ffff,
        0xff00_0000,
        0x1234_5678,
        0xabcd_ef01,
    ];
    let want: Vec<u32> = px.iter().map(|&p| mul_rgb(p, 300)).collect();
    scale_rgb(&mut px, 300);
    assert_eq!(px.to_vec(), want);
    assert!(px.iter().all(|p| p >> 24 == 0xff));
}

#[test]
fn lerp_endpoints_keep_or_replace_the_colour() {
    let mut keep = [0xff11_2233u32; 9];
    lerp_toward(&mut keep, 0xffaa_bbcc, 0);
    assert!(keep.iter().all(|&p| p == 0xff11_2233));
    let mut full = [0xff11_2233u32; 9];
    lerp_toward(&mut full, 0xffaa_bbcc, 256);
    assert!(full.iter().all(|&p| p == 0xffaa_bbcc));
}
