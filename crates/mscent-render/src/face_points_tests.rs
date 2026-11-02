use super::*;

/// A face in the plane z = 0 whose luxel (x, y) is the point (x, y).
fn flat_face() -> FaceProjection {
    FaceProjection {
        origin: Vec3::ZERO,
        tangent: Vec3::new(1.0, 0.0, 0.0),
        bitangent: Vec3::new(0.0, 1.0, 0.0),
        normal: Vec3::new(0.0, 0.0, 1.0),
        u_min: 0.0,
        v_min: 0.0,
        luxel_size: 1.0,
        width: 8,
        height: 8,
    }
}

fn one_light(at: Vec3, radius: f32) -> FacePoints {
    let mut p = FacePoints::NONE;
    assert!(p.add(&flat_face(), at, radius, Vec3::new(1.0, 1.0, 1.0)));
    p
}

#[test]
fn the_light_under_a_light_is_the_formula() {
    // Two units above the origin, radius 4: falloff (1 - 2/4)^2 and N.L = 1.
    let p = one_light(Vec3::new(0.0, 0.0, 2.0), 4.0);
    let c = p.at(0.0, 0.0);
    assert!((c.x - 64.0).abs() < 1e-3, "{c:?}");
    assert_eq!(c.x, c.y);
    // Three units across: distance sqrt(13), N.L = 2 / sqrt(13).
    let d = 13.0f32.sqrt();
    let want = 256.0 * (1.0 - d / 4.0).powi(2) * (2.0 / d);
    assert!((p.at(3.0, 0.0).x - want).abs() < 1e-3);
}

#[test]
fn nothing_beyond_the_radius_or_behind_the_face() {
    let p = one_light(Vec3::new(0.0, 0.0, 2.0), 4.0);
    assert_eq!(p.at(4.0, 0.0), Vec3::ZERO, "past the radius");
    let below = one_light(Vec3::new(0.0, 0.0, -2.0), 4.0);
    assert_eq!(below.at(0.0, 0.0), Vec3::ZERO, "behind the face");
}

#[test]
fn hostile_values_add_nothing() {
    let face = flat_face();
    let white = Vec3::new(1.0, 1.0, 1.0);
    let mut p = FacePoints::NONE;
    for (pos, radius, color) in [
        (Vec3::new(f32::NAN, 0.0, 1.0), 4.0, white),
        (Vec3::new(0.0, 0.0, f32::INFINITY), 4.0, white),
        (Vec3::new(0.0, 0.0, 1.0), f32::NAN, white),
        (Vec3::new(0.0, 0.0, 1.0), 0.0, white),
        (Vec3::new(0.0, 0.0, 1.0), -3.0, white),
        (Vec3::new(0.0, 0.0, 1.0), 4.0, Vec3::new(f32::NAN, 0.0, 0.0)),
    ] {
        assert!(
            !p.add(&face, pos, radius, color),
            "{pos:?} {radius} {color:?}"
        );
    }
    assert!(p.is_empty());
    // A NaN position on the face is a miss, not a NaN colour.
    let lit = one_light(Vec3::new(0.0, 0.0, 2.0), 4.0);
    assert_eq!(lit.at(f32::NAN, 0.0), Vec3::ZERO);
}

#[test]
fn at_most_four_lights_share_a_face() {
    let face = flat_face();
    let mut p = FacePoints::NONE;
    for _ in 0..MAX_DYN_LIGHTS {
        assert!(p.add(
            &face,
            Vec3::new(0.0, 0.0, 2.0),
            4.0,
            Vec3::new(0.1, 0.1, 0.1)
        ));
    }
    assert!(!p.add(
        &face,
        Vec3::new(0.0, 0.0, 2.0),
        4.0,
        Vec3::new(1.0, 1.0, 1.0)
    ));
    assert!((p.at(0.0, 0.0).x - 4.0 * 6.4).abs() < 1e-3, "four add up");
}

#[test]
fn a_span_steps_linearly_between_its_ends() {
    let p = one_light(Vec3::new(0.0, 0.0, 2.0), 4.0);
    let (c0, c1) = (p.at(0.0, 0.0), p.at(2.0, 0.0));
    let mut col = [[10.0, 20.0, 30.0, 0.0]; 3];
    p.add_span(&mut col, (0.0, 0.0), (2.0, 0.0), 2);
    assert!(
        (col[0][0] - (10.0 + c0.x)).abs() < 1e-3,
        "pixel 0 is the start"
    );
    assert!((col[1][0] - (10.0 + (c0.x + c1.x) / 2.0)).abs() < 1e-3);
    assert!(
        (col[2][0] - (10.0 + c1.x)).abs() < 1e-3,
        "pixel len is the end"
    );
    assert_eq!(col[0][3], 0.0, "the pad channel is left alone");
    // A span clear of the light leaves the colours as they were.
    let mut dark = [[1.0, 2.0, 3.0, 0.0]; 4];
    p.add_span(&mut dark, (20.0, 0.0), (24.0, 0.0), 4);
    assert_eq!(dark, [[1.0, 2.0, 3.0, 0.0]; 4]);
}
