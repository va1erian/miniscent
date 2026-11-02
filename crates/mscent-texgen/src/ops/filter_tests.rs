//! Behaviour of the colour filters and the combine operators.

use super::Value;
use super::test_util::{close, flat, grey_image, image_of, make, red, rgba, square};
use crate::image::Image;

fn f(v: f32) -> Value {
    Value::Float(v)
}

fn assert_rgba(got: [f32; 4], want: [f32; 4]) {
    assert!(
        got.iter().zip(want).all(|(g, w)| close(*g, w, 0.001)),
        "{got:?} against {want:?}"
    );
}

#[test]
fn color_multiplies_and_may_pass_the_input() {
    let src = flat(square(8), [0.25, 0.5, 0.75, 1.0]);
    let doubled = make("color", &[("amount", f(2.0))], &[&src], square(8));
    assert_rgba(rgba(&doubled, 3, 3), [0.5, 1.0, 1.0, 1.0]);
    let tinted = make(
        "color",
        &[("gain", Value::Color([1.0, 0.5, 0.0, 0.5]))],
        &[&src],
        square(8),
    );
    assert_rgba(rgba(&tinted, 0, 0), [0.25, 0.25, 0.0, 0.5]);
    let kept = make("color", &[], &[&src], square(8));
    assert_eq!(kept, src);
    // Past white is lost to the store, not wrapped around.
    let loud = make("color", &[("amount", f(8.0))], &[&src], square(8));
    assert_rgba(rgba(&loud, 0, 0), [1.0, 1.0, 1.0, 1.0]);
}

#[test]
fn range_maps_levels_and_applies_its_gamma() {
    let ramp = grey_image(square(16), |x, _| x as f32 / 15.0);
    let stretched = make(
        "range",
        &[("in_low", f(0.25)), ("in_high", f(0.75))],
        &[&ramp],
        square(16),
    );
    assert_eq!(red(&stretched, 0, 0), 0.0);
    assert_eq!(red(&stretched, 15, 0), 1.0);
    let halved = make("range", &[("out_high", f(0.5))], &[&ramp], square(16));
    assert!(close(red(&halved, 15, 0), 0.5, 0.001));
    let quarter = flat(square(4), [0.25, 0.25, 0.25, 1.0]);
    let bright = make("range", &[("gamma", f(2.0))], &[&quarter], square(4));
    assert!(
        close(red(&bright, 0, 0), 0.5, 0.005),
        "{}",
        red(&bright, 0, 0)
    );
    let swapped = make(
        "range",
        &[("out_low", f(1.0)), ("out_high", f(0.0))],
        &[&ramp],
        square(16),
    );
    assert_eq!((red(&swapped, 0, 0), red(&swapped, 15, 0)), (1.0, 0.0));
}

#[test]
fn range_with_an_empty_or_reversed_input_is_a_threshold() {
    let ramp = grey_image(square(16), |x, _| x as f32 / 15.0);
    for (low, high) in [(0.5, 0.5), (0.5, 0.2), (1.0, 0.0), (0.0, 0.0)] {
        let set = [("in_low", f(low)), ("in_high", f(high))];
        let out = make("range", &set, &[&ramp], square(16));
        for x in 0..16 {
            let want = if x as f32 / 15.0 >= low { 1.0 } else { 0.0 };
            assert_eq!(red(&out, x, 0), want, "{low}..{high} at {x}");
        }
    }
}

#[test]
fn invert_flips_colour_and_keeps_alpha() {
    let src = flat(square(4), [0.25, 0.5, 1.0, 0.5]);
    let out = make("invert", &[], &[&src], square(4));
    assert_rgba(rgba(&out, 1, 1), [0.75, 0.5, 0.0, 0.5]);
    assert_eq!(make("invert", &[], &[&out], square(4)), src);
}

#[test]
fn hscb_defaults_leave_the_picture_alone() {
    let src = flat(square(4), [0.2, 0.6, 0.9, 0.7]);
    let out = make("hscb", &[], &[&src], square(4));
    for (a, b) in out.pixels().iter().zip(src.pixels()) {
        assert!(a.iter().zip(b).all(|(p, q)| p.abs_diff(*q) <= 1));
    }
}

#[test]
fn hscb_hue_turns_red_to_green_a_third_of_the_way_round() {
    let red_px = flat(square(4), [1.0, 0.0, 0.0, 1.0]);
    let third = make("hscb", &[("hue", f(1.0 / 3.0))], &[&red_px], square(4));
    assert_rgba(rgba(&third, 0, 0), [0.0, 1.0, 0.0, 1.0]);
    let full = make("hscb", &[("hue", f(1.0))], &[&red_px], square(4));
    assert_rgba(rgba(&full, 0, 0), [1.0, 0.0, 0.0, 1.0]);
    let back = make("hscb", &[("hue", f(-1.0 / 3.0))], &[&red_px], square(4));
    assert_rgba(rgba(&back, 0, 0), [0.0, 0.0, 1.0, 1.0]);
}

#[test]
fn hscb_saturation_contrast_and_brightness() {
    let src = flat(square(4), [0.8, 0.4, 0.2, 1.0]);
    let grey = rgba(
        &make("hscb", &[("saturation", f(0.0))], &[&src], square(4)),
        0,
        0,
    );
    assert!(close(grey[0], grey[1], 0.001) && close(grey[1], grey[2], 0.001));
    let flatter = rgba(
        &make("hscb", &[("contrast", f(0.0))], &[&src], square(4)),
        0,
        0,
    );
    assert_rgba(flatter, [0.5, 0.5, 0.5, 1.0]);
    let dark = rgba(
        &make("hscb", &[("brightness", f(0.0))], &[&src], square(4)),
        0,
        0,
    );
    assert_rgba(dark, [0.0, 0.0, 0.0, 1.0]);
    let half = rgba(
        &make("hscb", &[("brightness", f(0.5))], &[&src], square(4)),
        0,
        0,
    );
    assert_rgba(half, [0.4, 0.2, 0.1, 1.0]);
}

#[test]
fn sharpen_overshoots_an_edge_and_leaves_flat_alone() {
    let step = grey_image(square(32), |x, _| if x < 16 { 0.3 } else { 0.7 });
    let out = make("sharpen", &[("radius", f(0.05))], &[&step], square(32));
    assert!(red(&out, 16, 5) > 0.72, "bright side {}", red(&out, 16, 5));
    assert!(red(&out, 15, 5) < 0.28, "dark side {}", red(&out, 15, 5));
    assert!(close(red(&out, 8, 5), 0.3, 0.005), "far from the edge");
    let none = make("sharpen", &[("amount", f(0.0))], &[&step], square(32));
    assert_eq!(none, step);
    let plain = flat(square(8), [0.4; 4]);
    assert_eq!(make("sharpen", &[], &[&plain], square(8)), plain);
}

fn mask_inputs() -> (Image, Image) {
    (
        flat(square(4), [0.0, 0.2, 0.4, 1.0]),
        flat(square(4), [1.0, 0.8, 0.6, 0.0]),
    )
}

#[test]
fn mask_extremes_pick_one_input_and_the_middle_mixes() {
    let (a, b) = mask_inputs();
    let at = |m: f32| flat(square(4), [m, 1.0 - m, 0.5, 0.0]);
    assert_eq!(make("mask", &[], &[&a, &b, &at(0.0)], square(4)), a);
    assert_eq!(make("mask", &[], &[&a, &b, &at(1.0)], square(4)), b);
    let mid = make("mask", &[], &[&a, &b, &at(0.5)], square(4));
    assert_rgba(rgba(&mid, 2, 2), [0.5, 0.5, 0.5, 0.5]);
}

#[test]
fn mask_reads_only_the_red_of_the_mask() {
    let (a, b) = mask_inputs();
    let green_only = flat(square(4), [0.0, 1.0, 1.0, 1.0]);
    assert_eq!(make("mask", &[], &[&a, &b, &green_only], square(4)), a);
}

fn paste_set(x: f32, y: f32, w: f32, h: f32) -> [(&'static str, Value); 4] {
    [("x", f(x)), ("y", f(y)), ("w", f(w)), ("h", f(h))]
}

#[test]
fn paste_puts_a_scaled_tile_in_a_hard_rectangle() {
    let bg = flat(square(64), [1.0, 0.0, 0.0, 1.0]);
    let tile = grey_image(square(64), |x, _| if x < 32 { 1.0 } else { 0.0 });
    let out = make(
        "paste",
        &paste_set(0.5, 0.5, 0.5, 0.5),
        &[&bg, &tile],
        square(64),
    );
    assert_eq!(
        rgba(&out, 2, 2),
        [1.0, 0.0, 0.0, 1.0],
        "outside is the background"
    );
    // The rectangle is x 16..48; the tile's left half fills its left half.
    assert_eq!(rgba(&out, 20, 32), [1.0, 1.0, 1.0, 1.0]);
    assert_eq!(rgba(&out, 44, 32), [0.0, 0.0, 0.0, 1.0]);
    assert_eq!(rgba(&out, 15, 32), [1.0, 0.0, 0.0, 1.0], "hard edge");
    assert_ne!(rgba(&out, 16, 32), [1.0, 0.0, 0.0, 1.0]);
}

#[test]
fn paste_wraps_and_respects_alpha() {
    let bg = flat(square(64), [1.0, 0.0, 0.0, 1.0]);
    let blue = flat(square(64), [0.0, 0.0, 1.0, 1.0]);
    let corner = make(
        "paste",
        &paste_set(0.0, 0.0, 0.25, 0.25),
        &[&bg, &blue],
        square(64),
    );
    for (x, y) in [(0, 0), (63, 0), (0, 63), (63, 63)] {
        assert_eq!(rgba(&corner, x, y), [0.0, 0.0, 1.0, 1.0], "{x},{y}");
    }
    let clear = flat(square(64), [0.0, 0.0, 1.0, 0.0]);
    assert_eq!(
        make(
            "paste",
            &paste_set(0.5, 0.5, 0.5, 0.5),
            &[&bg, &clear],
            square(64)
        ),
        bg
    );
    let half = flat(square(64), [0.0, 0.0, 1.0, 0.5]);
    let mixed = make(
        "paste",
        &paste_set(0.5, 0.5, 0.5, 0.5),
        &[&bg, &half],
        square(64),
    );
    assert_rgba(rgba(&mixed, 32, 32), [0.5, 0.0, 0.5, 1.0]);
    let none = make(
        "paste",
        &paste_set(0.5, 0.5, 0.0, 0.5),
        &[&bg, &blue],
        square(64),
    );
    assert_eq!(none, bg, "no width, no tile");
    let whole = make(
        "paste",
        &paste_set(0.5, 0.5, 1.0, 1.0),
        &[&bg, &blue],
        square(64),
    );
    assert_eq!(whole, blue);
}

#[test]
fn per_texel_filters_keep_inputs_alpha_where_they_say_so() {
    let src = image_of(square(4), |_, _| [0.5, 0.5, 0.5, 0.25]);
    for name in ["range", "hscb", "sharpen"] {
        assert_rgba(
            rgba(&make(name, &[], &[&src], square(4)), 0, 0),
            [0.5, 0.5, 0.5, 0.25],
        );
    }
}
