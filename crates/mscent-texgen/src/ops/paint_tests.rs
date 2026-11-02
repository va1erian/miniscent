//! What the paint tool relies on from the `paint` operator: that strokes dragged
//! across a tile's edge keep the tile seamless, and that a replay looks the same at
//! the size of a draft and the size of a full picture.

use std::sync::Arc;

use super::test_util::{image_of, make, red, square};
use super::{Frame, Image, Params, Stroke, Value, eval, find};
use crate::image::Size;

/// A path of points `step` apart along a curve from `from` to `to`, as the
/// editor records one: spaced by a fraction of the radius, not one per pixel.
fn path(from: [f32; 2], to: [f32; 2], bend: f32, step: f32) -> Vec<[f32; 2]> {
    let (dx, dy) = (to[0] - from[0], to[1] - from[1]);
    let count = ((dx * dx + dy * dy).sqrt() / step).ceil().max(1.0) as usize;
    (0..=count)
        .map(|i| {
            let t = i as f32 / count as f32;
            // A bow with no sine: a parabola, zero at both ends.
            let lift = bend * 4.0 * t * (1.0 - t);
            [from[0] + dx * t - dy * lift, from[1] + dy * t + dx * lift]
        })
        .collect()
}

fn brush(radius: f32, hardness: f32, color: [f32; 4], points: Vec<[f32; 2]>) -> Stroke {
    Stroke {
        radius,
        hardness,
        color,
        points,
    }
}

fn paint_with(list: Vec<Stroke>, input: Option<&Image>, size: Size, origin: [f32; 2]) -> Image {
    let spec = find("paint").expect("paint");
    let mut params = Params::defaults(spec);
    params
        .set(spec, "strokes", Value::Strokes(Arc::new(list)))
        .expect("strokes");
    let inputs: Vec<&Image> = input.into_iter().collect();
    eval(spec, &params, &inputs, Frame { size, origin }).expect("paints")
}

/// A hand's work: a thick soft stroke over the right edge and the bottom edge, a
/// thin hard one over the corner, and a dab.
fn hand_work() -> Vec<Stroke> {
    let white = [1.0, 1.0, 1.0, 1.0];
    vec![
        brush(
            0.06,
            0.3,
            [1.0, 0.4, 0.2, 1.0],
            path([0.8, 0.3], [1.3, 0.45], 0.1, 0.015),
        ),
        brush(0.02, 1.0, white, path([0.5, 0.8], [0.6, 1.4], -0.1, 0.005)),
        brush(
            0.03,
            0.8,
            [0.2, 0.6, 1.0, 0.5],
            path([0.9, 0.9], [1.2, 1.2], 0.0, 0.0075),
        ),
        brush(0.05, 0.5, white, vec![[0.25, 0.25]]),
    ]
}

fn periodic(size: Size) -> Image {
    // Repeats with the tile, and is not flat, so a wrong seam shows.
    let (w, h) = (size.width() as i32, size.height() as i32);
    image_of(size, move |x, y| {
        [
            (x * 3 % w) as f32 / w as f32,
            (y * 5 % h) as f32 / h as f32,
            0.3,
            1.0,
        ]
    })
}

fn moved_half(image: &Image) -> Image {
    let s = image.size();
    let (dx, dy) = (s.width() as i32 / 2, s.height() as i32 / 2);
    Image::from_xy(s, |x, y| image.at(x + dx, y + dy))
}

/// The operator's tiling test, with strokes over every edge: the window moved by
/// half a tile gives the picture moved by half a tile.
#[test]
fn strokes_over_the_edges_keep_the_tile_seamless() {
    let size = Size::new(128, 64).expect("size");
    let input = periodic(size);
    let here = paint_with(hand_work(), Some(&input), size, [0.0, 0.0]);
    let there = paint_with(hand_work(), Some(&moved_half(&input)), size, [0.5, 0.5]);
    let moved = moved_half(&here);
    let worst = moved
        .pixels()
        .iter()
        .zip(there.pixels())
        .flat_map(|(p, q)| (0..4).map(|c| (i32::from(p[c]) - i32::from(q[c])).abs()))
        .max()
        .unwrap_or(0);
    assert!(worst <= 96, "off by {worst} levels");
    // And the picture is not the plain input: the strokes are really there.
    assert_ne!(here, input);
}

/// A stroke that leaves by the right edge is on both sides of it, joined.
#[test]
fn a_stroke_over_the_edge_is_on_both_sides_of_it() {
    let line = brush(
        0.04,
        1.0,
        [1.0; 4],
        path([0.9, 0.5], [1.15, 0.5], 0.0, 0.01),
    );
    let img = make(
        "paint",
        &[("strokes", Value::Strokes(Arc::new(vec![line])))],
        &[],
        square(128),
    );
    assert_eq!((red(&img, 127, 64), red(&img, 0, 64)), (1.0, 1.0));
    assert_eq!((red(&img, 120, 64), red(&img, 15, 64)), (1.0, 1.0));
    assert_eq!(
        red(&img, 64, 64),
        0.0,
        "the middle of the tile is untouched"
    );
}

/// 8-bit levels of one channel of a texel, from the 16 bits it is kept in.
fn level(p: [u16; 4], channel: usize) -> f32 {
    f32::from(p[channel]) * 255.0 / 65535.0
}

/// The mean difference, in levels of 255, between a draft and a larger picture
/// averaged down to its size.
fn mean_difference(small: &Image, large: &Image) -> f32 {
    let factor = (large.size().width() / small.size().width()) as i32;
    let (w, h) = (small.size().width() as i32, small.size().height() as i32);
    let mut sum = 0.0;
    for y in 0..h {
        for x in 0..w {
            for c in 0..3 {
                let mut block = 0.0;
                for (dx, dy) in (0..factor * factor).map(|i| (i % factor, i / factor)) {
                    block += level(large.at(x * factor + dx, y * factor + dy), c);
                }
                let mean = block / (factor * factor) as f32;
                sum += (mean - level(small.at(x, y), c)).abs();
            }
        }
    }
    sum / (w * h * 3) as f32
}

#[test]
fn a_replay_at_128_and_512_agrees_within_two_levels_on_average() {
    let (small, large) = (
        paint_with(hand_work(), None, square(128), [0.0, 0.0]),
        paint_with(hand_work(), None, square(512), [0.0, 0.0]),
    );
    let difference = mean_difference(&small, &large);
    assert!(difference <= 2.0, "mean difference {difference} of 255");
    // The comparison would pass for two blank pictures: say that they are not.
    let ink = small.pixels().iter().filter(|p| p[0] > 20000).count();
    assert!(ink > 600, "{ink} inked texels at 128");
}
