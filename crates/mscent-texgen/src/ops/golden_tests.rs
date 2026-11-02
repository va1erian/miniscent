//! Golden bytes for what the default-parameter table cannot show: the table's
//! frame is small enough that `bricks` has no visible mortar and `paint` has no
//! strokes, so these cases are set up to draw something.

use std::sync::Arc;

use super::test_util::{make, square};
use super::{Stroke, Value};
use crate::image::Image;

fn fnv(image: &Image) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    for p in image.pixels() {
        for c in p {
            h = (h ^ u64::from(*c)).wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    h
}

fn strokes() -> Vec<Stroke> {
    let stroke = |hardness, points: &[[f32; 2]]| Stroke {
        radius: 0.06,
        hardness,
        color: [0.9, 0.6, 0.2, 0.8],
        points: points.to_vec(),
    };
    vec![
        stroke(0.5, &[[0.1, 0.2], [0.5, 0.8], [0.9, 0.3]]),
        stroke(1.0, &[[0.9, 0.9], [1.1, 1.0]]),
    ]
}

/// Change a value only for a kernel you meant to change.
#[test]
fn bricks_and_paint_make_their_golden_bytes() {
    let mode = |m| [("mode", Value::Choice(m))];
    let paint = [("strokes", Value::Strokes(Arc::new(strokes())))];
    let got = [
        (
            "bricks mask",
            fnv(&make("bricks", &mode(0), &[], square(256))),
        ),
        (
            "bricks tint",
            fnv(&make("bricks", &mode(1), &[], square(256))),
        ),
        (
            "bricks bevel",
            fnv(&make("bricks", &mode(2), &[], square(256))),
        ),
        ("paint", fnv(&make("paint", &paint, &[], square(64)))),
    ];
    let want = [
        0x97b7_29e6_5fb2_2325u64,
        0x18e0_87a6_3be1_2825,
        0xb659_e8a8_bdc6_6325,
        0x25f1_4806_cc6a_0dab,
    ];
    let wrong: Vec<String> = got
        .iter()
        .zip(want)
        .filter(|(g, w)| g.1 != *w)
        .map(|(g, _)| format!("(\"{}\", {:#x}),", g.0, g.1))
        .collect();
    assert!(
        wrong.is_empty(),
        "golden values should read:\n{}",
        wrong.join("\n")
    );
}
