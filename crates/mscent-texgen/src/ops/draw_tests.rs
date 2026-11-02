//! Behaviour of the drawing operators, read from real pixels.

use std::sync::Arc;

use super::draw::reaching;
use super::test_util::{close, flat, make, red, rgba, run, square, white_area};
use super::{MAX_POINTS, MAX_STROKES, Stroke, Value};
use crate::error::TexError;
use crate::image::Image;

fn f(v: f32) -> Value {
    Value::Float(v)
}

fn n(v: i32) -> Value {
    Value::Int(v)
}

#[test]
fn a_rect_covers_its_area_and_wraps_over_the_edge() {
    let img = make("rect", &[], &[], square(64));
    assert_eq!(red(&img, 32, 32), 1.0);
    assert_eq!(red(&img, 2, 2), 0.0);
    let area = white_area(&img);
    assert!(close(area, 0.5 * 0.25 * 4096.0, 6.0), "area {area}");
    let corner = make("rect", &[("x", f(0.0)), ("y", f(0.0))], &[], square(64));
    for (x, y) in [(0, 0), (63, 0), (0, 63), (63, 63)] {
        assert_eq!(red(&corner, x, y), 1.0, "{x},{y}");
    }
    assert_eq!(red(&corner, 32, 32), 0.0);
}

#[test]
fn shape_edges_are_anti_aliased_and_softness_widens_them() {
    let sharp = make("rect", &[("w", f(0.51))], &[], square(64));
    let soft = make(
        "rect",
        &[("w", f(0.51)), ("softness", f(0.2))],
        &[],
        square(64),
    );
    let ramp = |i: &Image| {
        (0..64)
            .filter(|&x| (0.02..0.98).contains(&red(i, x, 32)))
            .count()
    };
    assert!(ramp(&sharp) >= 1 && ramp(&sharp) <= 4, "{}", ramp(&sharp));
    assert!(ramp(&soft) > ramp(&sharp) + 6, "{}", ramp(&soft));
}

#[test]
fn shapes_draw_over_their_input_and_ignore_the_background_then() {
    let red_input = flat(square(32), [1.0, 0.0, 0.0, 1.0]);
    let blue_bg = [("background", Value::Color([0.0, 0.0, 1.0, 1.0]))];
    let over = make("rect", &blue_bg, &[&red_input], square(32));
    assert_eq!(
        rgba(&over, 0, 0),
        [1.0, 0.0, 0.0, 1.0],
        "outside shows the input"
    );
    assert_eq!(rgba(&over, 16, 16), [1.0, 1.0, 1.0, 1.0]);
    let bare = make("rect", &blue_bg, &[], square(32));
    assert_eq!(
        rgba(&bare, 0, 0),
        [0.0, 0.0, 1.0, 1.0],
        "no input: background"
    );
}

#[test]
fn an_ellipse_has_the_area_of_an_ellipse_and_wraps() {
    let img = make("ellipse", &[], &[], square(64));
    let want = core::f32::consts::PI * 0.25 * 0.125 * 4096.0;
    let area = white_area(&img);
    assert!(close(area, want, want * 0.04), "area {area}, want {want}");
    assert_eq!(red(&img, 4, 32), 0.0, "the corner of its box is empty");
    let edge = make("ellipse", &[("x", f(0.0))], &[], square(64));
    assert_eq!(red(&edge, 0, 32), 1.0);
    assert_eq!(red(&edge, 63, 32), 1.0);
}

#[test]
fn a_line_is_thick_between_its_ends_and_wraps() {
    let set = [
        ("x0", f(0.1)),
        ("y0", f(0.5)),
        ("x1", f(0.9)),
        ("y1", f(0.5)),
        ("width", f(0.1)),
    ];
    let img = make("line", &set, &[], square(64));
    assert_eq!(red(&img, 32, 32), 1.0);
    assert_eq!(red(&img, 32, 20), 0.0);
    assert_eq!(red(&img, 2, 32), 0.0, "short of its start");
    let on_edge = [
        ("x0", f(0.1)),
        ("y0", f(0.0)),
        ("x1", f(0.9)),
        ("y1", f(0.0)),
        ("width", f(0.1)),
    ];
    let top = make("line", &on_edge, &[], square(64));
    assert_eq!((red(&top, 32, 0), red(&top, 32, 63)), (1.0, 1.0));
    let dot = make("line", &[("x1", f(0.1)), ("y1", f(0.2))], &[], square(16));
    assert_eq!(dot.size(), square(16), "a zero-length line is a dab");
}

#[test]
fn dots_sit_in_the_middle_of_their_cells() {
    let set = [("count_x", n(4)), ("count_y", n(4))];
    let img = make("dots", &set, &[], square(64));
    for (cx, cy) in [(0, 0), (1, 2), (3, 3)] {
        assert_eq!(red(&img, cx * 16 + 8, cy * 16 + 8), 1.0, "rivet {cx},{cy}");
        assert_eq!(red(&img, cx * 16, cy * 16), 0.0, "between {cx},{cy}");
    }
    let want = 16.0 * core::f32::consts::PI * 4.0 * 4.0;
    assert!(close(white_area(&img), want, want * 0.05));
}

#[test]
fn a_neighbour_is_left_out_of_a_dots_search_only_when_it_cannot_reach() {
    // For every way a stamp can sit and every place in the cell, a cell that is
    // left out has its nearest possible stamp at least `reach` away.
    for slack in [0.0, 0.1, 0.3, 0.45] {
        for reach in [0.02, 0.1, 0.3, 0.6, 2.0] {
            for step in 0..100 {
                let f = step as f32 / 100.0;
                let (low, high) = reaching(f, slack, reach);
                assert!(low <= 0 && high >= 0, "the own cell is always searched");
                if low == 0 {
                    assert!(f + 0.5 - slack >= reach, "{f} {slack} {reach}");
                }
                if high == 0 {
                    assert!(1.5 - slack - f >= reach, "{f} {slack} {reach}");
                }
            }
        }
    }
}

#[test]
fn jittered_dots_move_but_stay_whole_and_follow_their_seed() {
    let set = |seed| {
        [
            ("count_x", n(4)),
            ("count_y", n(4)),
            ("radius", f(0.2)),
            ("jitter", f(1.0)),
            ("seed", n(seed)),
        ]
    };
    let plain = make(
        "dots",
        &[("count_x", n(4)), ("count_y", n(4)), ("radius", f(0.2))],
        &[],
        square(64),
    );
    let a = make("dots", &set(1), &[], square(64));
    assert_ne!(a, plain);
    assert_ne!(a, make("dots", &set(2), &[], square(64)));
    assert!(close(
        white_area(&a),
        white_area(&plain),
        white_area(&plain) * 0.08
    ));
}

fn stroke(points: &[[f32; 2]]) -> Stroke {
    Stroke {
        radius: 0.05,
        hardness: 1.0,
        color: [1.0, 1.0, 1.0, 1.0],
        points: points.to_vec(),
    }
}

fn strokes(list: Vec<Stroke>) -> [(&'static str, Value); 1] {
    [("strokes", Value::Strokes(Arc::new(list)))]
}

#[test]
fn a_stroke_crossing_an_edge_continues_on_the_other_side() {
    let set = strokes(vec![stroke(&[[0.9, 0.5], [1.1, 0.5]])]);
    let img = make("paint", &set, &[], square(64));
    assert_eq!((red(&img, 62, 32), red(&img, 1, 32)), (1.0, 1.0));
    assert_eq!(red(&img, 32, 32), 0.0);
    assert_eq!(red(&img, 62, 20), 0.0);
}

#[test]
fn strokes_replay_in_order() {
    let mut green = stroke(&[[0.5, 0.5]]);
    green.color = [0.0, 1.0, 0.0, 1.0];
    let mut magenta = stroke(&[[0.5, 0.5]]);
    magenta.color = [1.0, 0.0, 1.0, 1.0];
    let set = strokes(vec![green.clone(), magenta.clone()]);
    assert_eq!(
        rgba(&make("paint", &set, &[], square(32)), 16, 16),
        [1.0, 0.0, 1.0, 1.0]
    );
    let set = strokes(vec![magenta, green]);
    assert_eq!(
        rgba(&make("paint", &set, &[], square(32)), 16, 16),
        [0.0, 1.0, 0.0, 1.0]
    );
}

#[test]
fn a_translucent_stroke_is_not_doubled_where_its_segments_meet() {
    let mut s = stroke(&[[0.2, 0.2], [0.5, 0.5], [0.5, 0.5], [0.8, 0.5]]);
    s.color[3] = 0.5;
    let img = make("paint", &strokes(vec![s]), &[], square(64));
    let top = img.pixels().iter().map(|p| p[0]).max().unwrap_or(0);
    assert!(top <= 32768 + 1, "joints stay at the stroke's alpha: {top}");
    assert!(top >= 32000);
}

#[test]
fn a_stroke_looks_the_same_at_two_sizes_after_averaging() {
    let mut s = stroke(&[[0.1, 0.2], [0.5, 0.8], [0.9, 0.3]]);
    s.radius = 0.06;
    s.hardness = 0.5;
    let set = strokes(vec![s]);
    let (small, large) = (
        make("paint", &set, &[], square(64)),
        make("paint", &set, &[], square(256)),
    );
    let mut worst = 0.0f32;
    for y in 0..64 {
        for x in 0..64 {
            let mut sum = 0.0;
            for (dx, dy) in (0..16).map(|i| (i % 4, i / 4)) {
                sum += red(&large, x * 4 + dx, y * 4 + dy);
            }
            worst = worst.max((sum / 16.0 - red(&small, x, y)).abs());
        }
    }
    assert!(worst < 0.12, "worst block difference {worst}");
    let (a, b) = (white_area(&small) / 4096.0, white_area(&large) / 65536.0);
    assert!(close(a, b, 0.01), "coverage {a} against {b}");
}

#[test]
fn paint_refuses_what_exceeds_its_caps_or_is_not_a_number() {
    let bad = |list: Vec<Stroke>| run("paint", &strokes(list), &[], square(16));
    let dab = |r: f32| Stroke {
        radius: r,
        ..stroke(&[[0.5, 0.5]])
    };
    assert!(matches!(
        bad(vec![dab(0.0); MAX_STROKES + 1]),
        Err(TexError::BadParam(_))
    ));
    assert!(bad(vec![dab(0.0); MAX_STROKES]).is_ok());
    let long = stroke(&vec![[0.5, 0.5]; MAX_POINTS + 1]);
    assert!(matches!(bad(vec![long]), Err(TexError::BadParam(_))));
    let nan = stroke(&[[f32::NAN, 0.5]]);
    assert!(matches!(bad(vec![nan]), Err(TexError::BadParam(_))));
    let far = stroke(&[[1e30, 0.5]]);
    assert!(matches!(bad(vec![far]), Err(TexError::BadParam(_))));
    let fat = Stroke {
        radius: 4.0,
        ..stroke(&[[0.5, 0.5]])
    };
    assert!(matches!(bad(vec![fat]), Err(TexError::BadParam(_))));
    let infinite = Stroke {
        color: [f32::INFINITY, 0.0, 0.0, 1.0],
        ..stroke(&[[0.5, 0.5]])
    };
    assert!(matches!(bad(vec![infinite]), Err(TexError::BadParam(_))));
}

#[test]
fn paint_bounds_its_work() {
    // Thousands of tile-sized brushes would cost far more than a tile each.
    let huge = Stroke {
        radius: 0.5,
        ..stroke(&[[0.5, 0.5], [0.0, 0.0], [1.0, 1.0]])
    };
    let result = run("paint", &strokes(vec![huge; MAX_STROKES]), &[], square(64));
    assert!(matches!(result, Err(TexError::BadParam(_))));
    assert!(run("paint", &strokes(vec![]), &[], square(64)).is_ok());
}

#[test]
fn scratches_follow_their_seed_and_a_count_of_zero_draws_nothing() {
    let none = make("scratches", &[("count", n(0))], &[], square(64));
    assert!(none.pixels().iter().all(|p| *p == [0, 0, 0, 65535]));
    let a = make("scratches", &[], &[], square(64));
    assert_eq!(a, make("scratches", &[], &[], square(64)));
    assert_ne!(a, make("scratches", &[("seed", n(2))], &[], square(64)));
    let lit = a.pixels().iter().filter(|p| p[0] > 20000).count();
    assert!(lit > 40 && lit < 1500, "{lit} lit texels");
}

#[test]
fn scratches_wrap_over_the_edge() {
    // Enough long scratches that some must cross; the picture is then nearly
    // symmetric between opposite edges in lit-texel count.
    let set = [("count", n(512)), ("length", f(0.5)), ("width", f(0.02))];
    let img = make("scratches", &set, &[], square(64));
    let column = |x| (0..64).filter(|&y| red(&img, x, y) > 0.0).count();
    assert!(column(0) > 10 && column(63) > 10);
}

#[test]
fn text_draws_the_glyph_dots_and_wraps_off_the_edge() {
    let dots = |x: f32| {
        [
            ("text", Value::Text("L".into())),
            ("x", f(x)),
            ("y", f(0.0)),
            ("scale", f(0.0625)),
        ]
    };
    // Each font dot is 4 texels; L's first column is inked on all seven rows
    // and its bottom row across all five.
    let img = make("text", &dots(0.0), &[], square(64));
    assert_eq!(red(&img, 1, 1), 1.0);
    assert_eq!(
        red(&img, 5, 1),
        0.0,
        "second column of the top row is blank"
    );
    assert_eq!(red(&img, 1, 27), 1.0, "bottom row");
    assert_eq!(red(&img, 17, 25), 1.0);
    assert_eq!(red(&img, 1, 29), 0.0, "below the glyph");
    let wrapped = make("text", &dots(0.95), &[], square(64));
    assert_eq!((red(&wrapped, 62, 1), red(&wrapped, 0, 1)), (1.0, 1.0));
}

#[test]
fn text_leaves_unknown_characters_and_empty_text_blank() {
    for t in ["", "~", "\u{e9}", "   "] {
        let img = make("text", &[("text", Value::Text(t.into()))], &[], square(32));
        assert!(img.pixels().iter().all(|p| *p == [0, 0, 0, 65535]), "{t:?}");
    }
    let (lower, upper) = (
        make(
            "text",
            &[("text", Value::Text("abc".into()))],
            &[],
            square(64),
        ),
        make(
            "text",
            &[("text", Value::Text("ABC".into()))],
            &[],
            square(64),
        ),
    );
    assert_eq!(lower, upper);
    let long = "W".repeat(64);
    assert!(run("text", &[("text", Value::Text(long))], &[], square(32)).is_ok());
}
