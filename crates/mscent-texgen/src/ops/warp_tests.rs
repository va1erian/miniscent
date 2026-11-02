//! Behaviour of the warps and the height tools.

use super::Value;
use super::test_util::{close, flat, grey_image, image_of, make, red, square};
use crate::image::Image;
use crate::noise::{Fold, Noise};

fn f(v: f32) -> Value {
    Value::Float(v)
}

fn n(v: i32) -> Value {
    Value::Int(v)
}

/// A varied image of 32 texels square.
fn busy() -> Image {
    let noise = Noise {
        freq: (4, 4),
        octaves: 3,
        persistence: 0.5,
        seed: 5,
        fold: Fold::Fbm,
    };
    crate::Frame::new(square(32)).fill(|u, v| {
        let g = noise.at(u, v) * 0.5 + 0.5;
        [g, 1.0 - g, g * g, 1.0]
    })
}

fn corner() -> Image {
    grey_image(square(8), |x, y| if (x, y) == (0, 0) { 1.0 } else { 0.0 })
}

fn white_texel(img: &Image) -> Vec<(i32, i32)> {
    (0..8)
        .flat_map(|y| (0..8).map(move |x| (x, y)))
        .filter(|&(x, y)| red(img, x, y) > 0.9)
        .collect()
}

#[test]
fn rotozoom_quarter_turns_carry_a_known_texel_clockwise() {
    let src = corner();
    for (turns, at) in [
        (0, (0, 0)),
        (1, (7, 0)),
        (2, (7, 7)),
        (3, (0, 7)),
        (-1, (0, 7)),
        (4, (0, 0)),
        (-6, (7, 7)),
    ] {
        let out = make("rotozoom", &[("turns", n(turns))], &[&src], square(8));
        assert_eq!(white_texel(&out), vec![at], "{turns} quarter turns");
    }
}

#[test]
fn rotozoom_shift_moves_the_picture_and_zoom_repeats_it() {
    let src = grey_image(square(8), |x, y| if (x, y) == (3, 2) { 1.0 } else { 0.0 });
    let moved = make(
        "rotozoom",
        &[("shift_x", f(0.125)), ("shift_y", f(0.25))],
        &[&src],
        square(8),
    );
    assert_eq!(
        white_texel(&moved),
        vec![(2, 0)],
        "reads one right and two down"
    );
    let ramp = grey_image(square(8), |x, _| x as f32 / 7.0);
    let zoomed = make("rotozoom", &[("zoom_x", n(2))], &[&ramp], square(8));
    for x in 0..4 {
        assert!(
            close(red(&zoomed, x, 3), red(&zoomed, x + 4, 3), 0.002),
            "x {x}"
        );
    }
    let plain = make("rotozoom", &[], &[&src], square(8));
    assert_eq!(white_texel(&plain), white_texel(&src));
}

#[test]
fn twirl_leaves_everything_past_its_radius_alone() {
    let src = busy();
    let out = make("twirl", &[], &[&src], square(32));
    for (x, y) in [(0, 0), (31, 0), (0, 31), (31, 31), (16, 0), (0, 16)] {
        assert_eq!(out.at(x, y), src.at(x, y), "{x},{y}");
    }
    assert_ne!(out.at(20, 16), src.at(20, 16), "but not near the middle");
    let off = make("twirl", &[("strength", f(0.0))], &[&src], square(32));
    let off_by = off
        .pixels()
        .iter()
        .zip(src.pixels())
        .map(|(a, b)| a[0].abs_diff(b[0]))
        .max();
    assert!(off_by <= Some(2), "no strength, no change: {off_by:?}");
    assert_eq!(
        make("twirl", &[("radius", f(0.0))], &[&src], square(32)),
        src
    );
}

#[test]
fn twirl_turns_the_picture_the_way_its_sign_says() {
    // A bright texel right of centre moves up for a positive strength,
    // and down for a negative one, because y runs down.
    let src = image_of(square(64), |x, y| {
        let on = (x - 40).abs() <= 1 && (y - 32).abs() <= 1;
        [f32::from(u8::from(on)); 4]
    });
    let centroid_y = |img: &Image| {
        let (mut sum, mut mass) = (0.0, 0.0);
        for y in 0..64 {
            for x in 0..64 {
                let w = red(img, x, y);
                sum += w * y as f32;
                mass += w;
            }
        }
        sum / mass
    };
    let set = |s| [("strength", f(s)), ("radius", f(0.45))];
    let (a, b) = (
        make("twirl", &set(1.0), &[&src], square(64)),
        make("twirl", &set(-1.0), &[&src], square(64)),
    );
    assert!((centroid_y(&a) - centroid_y(&b)).abs() > 2.0);
    assert!(centroid_y(&a) < centroid_y(&b));
}

#[test]
fn bulge_magnifies_for_a_positive_strength_and_pinches_for_a_negative_one() {
    let ramp = grey_image(square(64), |x, _| x as f32 / 63.0);
    let swell = make(
        "bulge",
        &[("strength", f(1.0)), ("radius", f(0.5))],
        &[&ramp],
        square(64),
    );
    let pinch = make(
        "bulge",
        &[("strength", f(-1.0)), ("radius", f(0.5))],
        &[&ramp],
        square(64),
    );
    // Right of the centre a magnifier reads nearer the centre, so darker.
    assert!(red(&swell, 44, 32) < red(&ramp, 44, 32) - 0.05);
    assert!(red(&pinch, 44, 32) > red(&ramp, 44, 32) + 0.05);
    assert!(close(red(&swell, 32, 32), red(&ramp, 32, 32), 0.03));
    for img in [&swell, &pinch] {
        assert_eq!(img.at(0, 0), ramp.at(0, 0), "corner");
        assert_eq!(img.at(63, 63), ramp.at(63, 63), "corner");
    }
}

#[test]
fn distort_reads_where_the_map_points() {
    let src = grey_image(square(64), |x, y| if (x, y) == (20, 5) { 1.0 } else { 0.0 });
    let right = flat(square(64), [1.0, 0.5, 0.0, 1.0]);
    let out = make(
        "distort",
        &[("amount", f(0.5))],
        &[&src, &right],
        square(64),
    );
    // Red 1.0 reads a quarter tile to the right: 16 texels.
    assert!(close(red(&out, 4, 5), 1.0, 0.01));
    let left = flat(square(64), [0.0, 0.5, 0.0, 1.0]);
    let out = make("distort", &[("amount", f(0.5))], &[&src, &left], square(64));
    assert!(close(red(&out, 36, 5), 1.0, 0.01));
    let still = flat(square(64), [0.5, 0.5, 0.0, 1.0]);
    let out = make("distort", &[], &[&src, &still], square(64));
    assert!(close(red(&out, 20, 5), 1.0, 0.01));
    let none = make(
        "distort",
        &[("amount", f(0.0))],
        &[&src, &right],
        square(64),
    );
    assert!(close(red(&none, 20, 5), 1.0, 0.01));
}

#[test]
fn distort_moves_vertically_by_the_green_channel() {
    let src = grey_image(square(64), |x, y| if (x, y) == (7, 30) { 1.0 } else { 0.0 });
    let down = flat(square(64), [0.5, 1.0, 0.0, 1.0]);
    let out = make("distort", &[("amount", f(0.5))], &[&src, &down], square(64));
    assert!(close(red(&out, 7, 14), 1.0, 0.01));
    assert!(red(&out, 7, 30) < 0.01);
}
