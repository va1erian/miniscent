//! Behaviour of the generators, read from real pixels.

use super::Value;
use super::test_util::{close, make, red, rgba, run, square, white_area};
use crate::error::TexError;
use crate::image::Size;

fn int(v: i32) -> Value {
    Value::Int(v)
}

fn float(v: f32) -> Value {
    Value::Float(v)
}

const BLACK: Value = Value::Color([0.0, 0.0, 0.0, 1.0]);
const WHITE: Value = Value::Color([1.0, 1.0, 1.0, 1.0]);

fn mode(m: u8) -> (&'static str, Value) {
    ("mode", Value::Choice(m))
}

#[test]
fn bricks_mortar_is_the_same_world_length_on_both_axes() {
    // 8 rows: a brick is 32 texels high at 256, and the mortar a sixteenth of
    // that, 2 texels, between rows and between bricks alike.
    let img = make("bricks", &[], &[], square(256));
    let down = (0..256).filter(|&y| red(&img, 48, y) == 0.0).count();
    assert_eq!(down, 8 * 2, "eight joints of two texels");
    let along = (0..256).filter(|&x| red(&img, x, 16) == 0.0).count();
    assert_eq!(along, 4 * 2, "four joints of two texels");
}

#[test]
fn odd_rows_stagger_by_the_offset() {
    let img = make("bricks", &[], &[], square(256));
    // Row 0 has its upright joint at x = 64; row 1, half a brick over, at 32.
    assert_eq!((red(&img, 63, 16), red(&img, 64, 16)), (0.0, 0.0));
    assert_eq!((red(&img, 63, 48), red(&img, 64, 48)), (1.0, 1.0));
    assert_eq!((red(&img, 31, 48), red(&img, 32, 48)), (0.0, 0.0));
}

#[test]
fn bricks_that_cannot_alternate_are_refused() {
    let odd = [("rows", int(7))];
    assert_eq!(
        run("bricks", &odd, &[], square(64)),
        Err(TexError::NotTiling("odd brick rows cannot alternate"))
    );
    let unshifted = [("rows", int(7)), ("offset", float(0.0))];
    assert!(run("bricks", &unshifted, &[], square(64)).is_ok());
    assert!(run("bricks", &[("rows", int(6))], &[], square(64)).is_ok());
}

#[test]
fn the_three_bricks_modes_share_one_layout() {
    let s = square(256);
    let (mask, tint, bevel) = (
        make("bricks", &[mode(0)], &[], s),
        make("bricks", &[mode(1)], &[], s),
        make("bricks", &[mode(2)], &[], s),
    );
    for y in 0..256 {
        for x in 0..256 {
            let (m, t, b) = (red(&mask, x, y), red(&tint, x, y), red(&bevel, x, y));
            if m == 0.0 {
                assert_eq!((t, b), (0.0, 0.0), "mortar at {x},{y}");
            } else {
                assert!(t >= 0.29, "tint brick at {x},{y}: {t}");
            }
        }
    }
    let same_brick = (red(&tint, 70, 5), red(&tint, 120, 25));
    assert_eq!(same_brick.0, same_brick.1);
    assert_ne!(
        red(&tint, 70, 5),
        red(&tint, 140, 5),
        "next brick, other grey"
    );
}

#[test]
fn a_bricks_tint_follows_row_and_column_not_position() {
    // The same counts at another size give the same greys per brick.
    let (a, b) = (
        make("bricks", &[mode(1)], &[], square(64)),
        make("bricks", &[mode(1)], &[], square(256)),
    );
    assert_eq!(red(&a, 8, 4), red(&b, 32, 16));
    assert_eq!(red(&a, 40, 4), red(&b, 160, 16));
    let reseeded = make("bricks", &[mode(1), ("seed", int(2))], &[], square(64));
    assert_ne!(red(&a, 8, 4), red(&reseeded, 8, 4));
}

#[test]
fn bevel_ramps_from_the_mortar_up_to_the_top() {
    let img = make("bricks", &[mode(2)], &[], square(256));
    let row: Vec<f32> = (64..=96).map(|x| red(&img, x, 16)).collect();
    assert_eq!(row[0], 0.0, "mortar");
    assert!(
        row[2] > 0.0 && row[2] < 0.5,
        "foot of the bevel: {}",
        row[2]
    );
    assert!(row.windows(2).all(|w| w[0] <= w[1]), "rises: {row:?}");
    assert_eq!(row[32], 1.0, "flat top");
}

#[test]
fn checker_alternates_by_cell_and_refuses_odd_counts() {
    let img = make("checker", &[], &[], Size::new(64, 32).expect("size"));
    // 8 columns over 64 texels, 8 rows over 32.
    assert_eq!(red(&img, 0, 0), 0.0);
    assert_eq!(red(&img, 8, 0), 1.0);
    assert_eq!(red(&img, 0, 4), 1.0);
    assert_eq!(red(&img, 8, 4), 0.0);
    assert_eq!(white_area(&img), 64.0 * 32.0 / 2.0);
    for (key, odd) in [("cols", 7), ("rows", 3)] {
        assert!(matches!(
            run("checker", &[(key, int(odd))], &[], square(64)),
            Err(TexError::NotTiling(_))
        ));
    }
}

#[test]
fn vertical_stripes_are_hard_on_texel_boundaries() {
    let set = [("cycles_y", int(0)), ("color0", BLACK), ("color1", WHITE)];
    let img = make("stripes", &set, &[], square(64));
    // Four cycles over 64 texels: 8 lit, 8 dark.
    for x in 0..16 {
        let want = if x < 8 { 1.0 } else { 0.0 };
        assert!(
            close(red(&img, x, 3), want, 0.01),
            "x {x}: {}",
            red(&img, x, 3)
        );
    }
}

#[test]
fn diagonal_stripes_are_anti_aliased_and_keep_their_duty() {
    for duty in [0.25f32, 0.5, 0.8] {
        let set = [
            ("cycles_x", int(3)),
            ("cycles_y", int(2)),
            ("duty", float(duty)),
        ];
        let img = make("stripes", &set, &[], square(64));
        let mean = white_area(&img) / 4096.0;
        assert!(close(mean, duty, 0.002), "duty {duty}: mean {mean}");
        let between = img
            .pixels()
            .iter()
            .filter(|p| p[0] > 3000 && p[0] < 62000)
            .count();
        assert!(
            between > 100,
            "edges are not stepped: {between} soft texels"
        );
    }
}

#[test]
fn stripes_with_no_cycles_are_the_first_colour() {
    let set = [("cycles_x", int(0)), ("cycles_y", int(0))];
    let img = make("stripes", &set, &[], square(32));
    assert!(img.pixels().iter().all(|p| *p == [0, 0, 0, 65535]));
}

#[test]
fn gradient_goes_up_and_comes_back() {
    let img = make("gradient", &[], &[], square(64));
    assert!(red(&img, 31, 0) > 0.98 && red(&img, 32, 0) > 0.98);
    assert!(close(red(&img, 0, 0), red(&img, 63, 0), 0.001));
    assert!(red(&img, 0, 0) < 0.03);
    let (rising, falling): (Vec<f32>, Vec<f32>) = (
        (0..32).map(|x| red(&img, x, 9)).collect(),
        (32..64).map(|x| red(&img, x, 9)).collect(),
    );
    assert!(rising.windows(2).all(|w| w[0] <= w[1]));
    assert!(falling.windows(2).all(|w| w[0] >= w[1]));
    let down = make(
        "gradient",
        &[("cycles_x", int(0)), ("cycles_y", int(-1))],
        &[],
        square(64),
    );
    assert!(close(red(&down, 5, 31), red(&down, 5, 32), 0.001));
}

#[test]
fn glow_is_brightest_at_its_centre_and_wraps() {
    let img = make("glow", &[], &[], square(64));
    assert!(red(&img, 32, 32) > 0.95);
    assert_eq!(red(&img, 0, 0), 0.0, "past the radius");
    let corner = make(
        "glow",
        &[("x", float(0.0)), ("y", float(0.0))],
        &[],
        square(64),
    );
    for (x, y) in [(0, 0), (63, 0), (0, 63), (63, 63)] {
        assert!(red(&corner, x, y) > 0.9, "{x},{y}");
    }
    assert_eq!(red(&corner, 32, 32), 0.0);
}

#[test]
fn glow_power_bends_the_falloff_and_a_zero_radius_is_all_outer() {
    let (soft, hard) = (
        make("glow", &[("power", float(0.5))], &[], square(64)),
        make("glow", &[("power", float(4.0))], &[], square(64)),
    );
    assert!(red(&soft, 48, 32) < red(&hard, 48, 32));
    let none = make("glow", &[("radius", float(0.0))], &[], square(16));
    assert!(none.pixels().iter().all(|p| *p == [0, 0, 0, 65535]));
}

#[test]
fn cells_modes_make_three_different_pictures() {
    let s = square(64);
    let (distance, edge, id) = (
        make("cells", &[mode(0)], &[], s),
        make("cells", &[mode(1)], &[], s),
        make("cells", &[mode(2)], &[], s),
    );
    assert_ne!(distance, edge);
    assert_ne!(distance, id);
    assert_ne!(edge, id);
    let min = |i: &crate::Image| i.pixels().iter().map(|p| p[0]).min().unwrap_or(0);
    assert!(min(&distance) < 6000, "a texel sits near a point");
    assert!(min(&edge) < 6000, "and one on a border");
    let mut greys: Vec<u16> = id.pixels().iter().map(|p| p[0]).collect();
    greys.sort_unstable();
    greys.dedup();
    assert!(
        (8..=16).contains(&greys.len()),
        "one grey per cell: {}",
        greys.len()
    );
}

#[test]
fn cells_follow_their_seed_and_survive_odd_counts() {
    let s = square(64);
    let a = make("cells", &[], &[], s);
    assert_eq!(a, make("cells", &[], &[], s));
    assert_ne!(a, make("cells", &[("seed", int(2))], &[], s));
    for count in [1, 2, 3, 17, 255, 256] {
        let img = make("cells", &[("count", int(count)), mode(2)], &[], s);
        assert_eq!(img.size(), s);
    }
    let mut greys: Vec<u16> = make("cells", &[("count", int(256)), mode(2)], &[], square(256))
        .pixels()
        .iter()
        .map(|p| p[0])
        .collect();
    greys.sort_unstable();
    greys.dedup();
    assert!(greys.len() > 200 && greys.len() <= 256, "{}", greys.len());
}

#[test]
fn wavelet_varies_stays_in_range_and_follows_its_seed() {
    let s = square(64);
    let a = make("wavelet", &[], &[], s);
    let mean = white_area(&a) / 4096.0;
    assert!(close(mean, 0.5, 0.1), "mean {mean}");
    let spread = a.pixels().iter().map(|p| p[0]).max().unwrap_or(0)
        - a.pixels().iter().map(|p| p[0]).min().unwrap_or(0);
    assert!(spread > 30000, "spread {spread}");
    assert_ne!(a, make("wavelet", &[("seed", int(9))], &[], s));
    assert_eq!(rgba(&a, 0, 0)[3], 1.0);
}
