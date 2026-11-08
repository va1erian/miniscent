//! Projectors: the file, the text, the validity rules and the light they bake.

use crate::brush::{Brush, Op};
use crate::csg::compile;
use crate::entity::{Entity, Level, projector_dir};
use crate::format::{self, FormatError};
use crate::level_text::{from_text, to_text};
use crate::lightmap::{BakeConfig, Cone, LightFace, Lightmap, PointLight, bake};
use mscent_math::{Vec3, Vec3d};

fn v3(x: f32, y: f32, z: f32) -> Vec3 {
    Vec3::new(x, y, z)
}

fn projector() -> Entity {
    Entity::Projector {
        pos: v3(1.0, 2.0, 3.0),
        yaw: 0.5,
        pitch: -1.0,
        color: v3(1.0, 0.9, 0.8),
        radius: 20.0,
        intensity: 1.5,
        inner: 0.2,
        outer: 0.6,
    }
}

fn room_with(e: Entity) -> Level {
    let b = Brush::cuboid(
        Vec3d::new(-8.0, -8.0, -8.0),
        Vec3d::new(8.0, 8.0, 8.0),
        Op::Subtract,
        0,
    )
    .unwrap();
    Level {
        brushes: vec![b],
        entities: vec![e],
        ..Default::default()
    }
}

/// `projector()` with one field changed by `f`.
fn tweaked(f: impl FnOnce(&mut Entity)) -> Entity {
    let mut e = projector();
    f(&mut e);
    e
}

#[test]
fn a_projector_round_trips_in_binary_and_in_text() {
    let level = room_with(projector());
    assert_eq!(format::read(&format::write(&level)).unwrap(), level);
    let text = to_text(&level);
    assert!(text.contains("projector 1 2 3"), "{text}");
    assert_eq!(from_text(&text).unwrap(), level);
}

#[test]
fn a_version_5_file_cannot_hold_one() {
    let mut bytes = format::write(&room_with(projector()));
    bytes[4..6].copy_from_slice(&5u16.to_le_bytes());
    assert_eq!(format::read(&bytes), Err(FormatError::BadEntityTag(8)));
}

#[test]
fn a_flipped_bit_or_a_cut_never_panics() {
    let bytes = format::write(&room_with(projector()));
    for i in 0..bytes.len() {
        let mut flipped = bytes.clone();
        flipped[i] ^= 0x80;
        let _ = format::read(&flipped);
        let _ = format::read(&bytes[..i]);
    }
}

#[test]
fn hostile_projectors_are_rejected() {
    let nan = f32::NAN;
    let cases = [
        tweaked(|e| set(e, |y, _, _, _, _| *y = nan)),
        tweaked(|e| set(e, |_, p, _, _, _| *p = 1.7)),
        tweaked(|e| set(e, |_, p, _, _, _| *p = nan)),
        tweaked(|e| set(e, |_, _, i, _, _| *i = 0.7)),
        tweaked(|e| set(e, |_, _, i, _, _| *i = -0.1)),
        tweaked(|e| set(e, |_, _, _, o, _| *o = 1.6)),
        tweaked(|e| set(e, |_, _, _, o, _| *o = 0.0)),
        tweaked(|e| set(e, |_, _, _, _, r| *r = 0.0)),
        tweaked(|e| set(e, |_, _, _, _, r| *r = f32::INFINITY)),
    ];
    for e in cases {
        assert!(!e.is_valid(), "{e:?}");
        let r = format::read(&format::write(&room_with(e)));
        assert_eq!(r, Err(FormatError::BadEntity), "{e:?}");
    }
}

/// Edits `yaw`, `pitch`, `inner`, `outer` and `radius` of a projector.
fn set(e: &mut Entity, f: impl FnOnce(&mut f32, &mut f32, &mut f32, &mut f32, &mut f32)) {
    if let Entity::Projector {
        yaw,
        pitch,
        inner,
        outer,
        radius,
        ..
    } = e
    {
        f(yaw, pitch, inner, outer, radius);
    }
}

#[test]
fn bad_projector_lines_are_refused() {
    let head = "miniscent-level 1\n";
    for line in [
        "projector 0 0 0  0 -1  1 1 1  10 1  0.7 0.5",
        "projector 0 0 0  0 -2  1 1 1  10 1  0.1 0.5",
        "projector 0 0 0  0 -1  1 1 1  10 1  0.1",
        "projector 0 0 0  0 -1  1 1 2  10 1  0.1 0.5",
    ] {
        assert!(from_text(&format!("{head}{line}\n")).is_err(), "{line}");
    }
}

#[test]
fn the_direction_follows_yaw_and_pitch() {
    let close = |a: Vec3, b: Vec3| (a - b).length() < 1e-5;
    assert!(close(projector_dir(0.0, 0.0), v3(1.0, 0.0, 0.0)));
    assert!(close(
        projector_dir(core::f32::consts::FRAC_PI_2, 0.0),
        v3(0.0, 1.0, 0.0)
    ));
    assert!(close(
        projector_dir(1.0, -core::f32::consts::FRAC_PI_2),
        v3(0.0, 0.0, -1.0)
    ));
}

/// A 20 by 20 floor at z 0, in a room, lit by a downward projector at (10, 10, 6)
/// with half angles 0.2 and 0.5 (about 11 and 29 degrees), with a plate under it
/// when `plate` says so.
fn pool(plate: bool) -> Lightmap {
    let cube = |lo: [f64; 3], hi: [f64; 3], op, order| {
        Brush::cuboid(
            Vec3d::new(lo[0], lo[1], lo[2]),
            Vec3d::new(hi[0], hi[1], hi[2]),
            op,
            order,
        )
        .unwrap()
    };
    let mut brushes = vec![cube([0.0, 0.0, 0.0], [20.0, 20.0, 12.0], Op::Subtract, 0)];
    if plate {
        brushes.push(cube([9.0, 9.0, 3.0], [11.0, 11.0, 3.3], Op::Add, 1));
    }
    let bsp = compile(&brushes).unwrap();
    let floor = LightFace {
        vertices: vec![
            v3(0.0, 0.0, 0.0),
            v3(20.0, 0.0, 0.0),
            v3(20.0, 20.0, 0.0),
            v3(0.0, 20.0, 0.0),
        ],
        normal: v3(0.0, 0.0, 1.0),
        density: Default::default(),
    };
    let lamp = PointLight {
        position: v3(10.0, 10.0, 6.0),
        color: v3(1.0, 1.0, 1.0),
        radius: 40.0,
        cone: Cone::new(v3(0.0, 0.0, -1.0), 0.2, 0.5),
    };
    let cfg = BakeConfig {
        luxel_size: 0.25,
        ambient: Vec3::ZERO,
        albedo: 0.0,
        ..BakeConfig::default()
    };
    bake(&bsp, &[floor], &[lamp], &cfg).unwrap()
}

/// Light on the floor `d` from the spot under the lamp, along x.
fn at(map: &Lightmap, d: f32) -> f32 {
    map.sample_world(0, v3(10.0 + d, 10.0, 0.0))
        .unwrap()
        .color
        .x
}

#[test]
fn light_falls_in_the_cone_and_not_outside_it() {
    let map = pool(false);
    // The lamp is 6 up: the inner edge meets the floor at 6 tan 0.2 = 1.2 and the
    // outer at 6 tan 0.5 = 3.3.
    let (centre, inside, rim, outside, far) = (
        at(&map, 0.0),
        at(&map, 1.0),
        at(&map, 2.3),
        at(&map, 3.6),
        at(&map, 7.0),
    );
    assert!(centre > 0.1, "centre {centre}");
    assert!(inside > 0.5 * centre, "inside {inside} of {centre}");
    assert!(
        rim > 0.0 && rim < inside,
        "rim {rim} between 0 and {inside}"
    );
    assert_eq!(outside, 0.0);
    assert_eq!(far, 0.0);
}

#[test]
fn the_pool_is_round() {
    let map = pool(false);
    let point = |dx: f32, dy: f32| {
        map.sample_world(0, v3(10.0 + dx, 10.0 + dy, 0.0))
            .unwrap()
            .color
            .x
    };
    let (a, b) = (point(2.3, 0.0), point(0.0, 2.3));
    assert!((a - b).abs() < 0.02 * a.max(b), "{a} against {b}");
    assert!(
        point(2.5, 2.5) == 0.0,
        "past the outer angle on the diagonal"
    );
}

#[test]
fn a_wall_in_the_beam_still_throws_its_shadow() {
    let open = pool(false);
    let shaded = pool(true);
    // The plate is 2 wide at 3 up under a lamp at 6: its shadow on the floor is
    // 4 wide, so the centre is dark and the rim of the pool is not.
    assert!(at(&open, 0.0) > 0.1);
    assert!(at(&shaded, 0.0) < 0.01 * at(&open, 0.0));
    let rim_open = at(&open, 2.8);
    let rim_shaded = at(&shaded, 2.8);
    assert!((rim_open - rim_shaded).abs() < 0.05 * rim_open.max(0.001));
}
