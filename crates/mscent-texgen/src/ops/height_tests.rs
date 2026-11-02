//! Behaviour of the height tools.

use super::Value;
use super::test_util::{close, flat, grey_image, make, red, square};

fn f(v: f32) -> Value {
    Value::Float(v)
}

#[test]
fn bump_of_a_flat_height_is_the_albedo() {
    let albedo = grey_image(square(16), |x, y| (x + y) as f32 / 40.0);
    let height = flat(square(16), [0.4, 0.4, 0.4, 1.0]);
    assert_eq!(make("bump", &[], &[&albedo, &height], square(16)), albedo);
}

#[test]
fn bump_lights_a_slope_from_the_side_the_light_is_on() {
    let albedo = flat(square(64), [0.5, 0.5, 0.5, 1.0]);
    let ramp = grey_image(square(64), |x, _| x as f32 / 63.0);
    let lit = |lx: f32| {
        let set = [
            ("light_x", f(lx)),
            ("light_y", f(0.0)),
            ("strength", f(32.0)),
        ];
        red(&make("bump", &set, &[&albedo, &ramp], square(64)), 32, 32)
    };
    // The height rises to the right, so the surface faces left.
    assert!(lit(-1.0) > 0.5 + 0.03, "light from the left: {}", lit(-1.0));
    assert!(lit(1.0) < 0.5 - 0.03, "light from the right: {}", lit(1.0));
    assert!(close(
        lit(0.0),
        0.5 * (0.5 + 0.5 / (1.0f32 + 0.254 * 0.254).sqrt()),
        0.01
    ));
}

#[test]
fn bump_without_strength_or_with_full_ambient_changes_nothing() {
    let albedo = grey_image(square(32), |x, _| x as f32 / 40.0);
    let relief = grey_image(square(32), |x, y| ((x * 7 + y * 3) % 11) as f32 / 10.0);
    assert_eq!(
        make(
            "bump",
            &[("strength", f(0.0))],
            &[&albedo, &relief],
            square(32)
        ),
        albedo
    );
    let set = [("ambient", f(1.0)), ("strength", f(32.0))];
    assert_eq!(make("bump", &set, &[&albedo, &relief], square(32)), albedo);
}

#[test]
fn bump_looks_the_same_at_two_sizes() {
    let at = |side: u32| {
        let height = make("gradient", &[], &[], square(side));
        let albedo = flat(square(side), [0.5, 0.5, 0.5, 1.0]);
        let set = [("light_x", f(-1.0)), ("strength", f(8.0))];
        let out = make("bump", &set, &[&albedo, &height], square(side));
        let s = side as i32;
        red(&out, s / 4, s / 2)
    };
    let (small, large) = (at(64), at(256));
    assert!(small > 0.52, "there is relief to compare: {small}");
    assert!(close(small, large, 0.005), "{small} against {large}");
}

#[test]
fn cavity_darkens_only_below_the_surroundings() {
    let flat_height = flat(square(32), [0.7, 0.7, 0.7, 1.0]);
    let none = make("cavity", &[], &[&flat_height], square(32));
    assert!(none.pixels().iter().all(|p| *p == [65535; 4]));
    let pitted = grey_image(square(32), |x, y| {
        if (12..20).contains(&x) && (12..20).contains(&y) {
            0.2
        } else {
            0.8
        }
    });
    let set = [("radius", f(0.1)), ("amount", f(2.0))];
    let out = make("cavity", &set, &[&pitted], square(32));
    assert!(
        red(&out, 16, 16) < 0.7,
        "inside the pit: {}",
        red(&out, 16, 16)
    );
    assert_eq!(red(&out, 2, 2), 1.0, "far from it");
    assert_eq!(red(&out, 10, 16), 1.0, "a rim is not a crevice");
    let off = make("cavity", &[("amount", f(0.0))], &[&pitted], square(32));
    assert!(off.pixels().iter().all(|p| *p == [65535; 4]));
}

#[test]
fn slope_is_black_when_flat_and_steady_across_sizes() {
    let level = flat(square(16), [0.3, 0.3, 0.3, 1.0]);
    assert!(
        make("slope", &[], &[&level], square(16))
            .pixels()
            .iter()
            .all(|p| p[0] == 0)
    );
    let at = |side: u32| {
        let height = make("gradient", &[], &[], square(side));
        let out = make("slope", &[], &[&height], square(side));
        red(&out, side as i32 / 4, side as i32 / 2)
    };
    // A triangle with slope 2 per tile, at strength 4 and 128 px: 2 * 4 / 128.
    let (small, large) = (at(64), at(256));
    assert!(close(small, 0.0625, 0.002), "{small}");
    assert!(close(large, 0.0625, 0.002), "{large}");
}
