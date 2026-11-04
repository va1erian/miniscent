//! The player's ship as the GPU draws it: in first person it is never drawn, and it
//! still casts. Nothing here compares with software, which draws no shadows.
//!
//! The ship is the game's `assets/ship/player.msb`, placed as the game places it:
//! turned as a player facing `yaw`, at the player's position. Here at twice its size
//! and hung 3 below a light, so that its shadow is wider than the soft edge.
//!
//! By hand, with the light at (0, 0, 3), the ship at (0, 0, -1) facing +Y and the
//! floor at -4: a point of the ship `d` below the light throws its shadow
//! `7 / d` times as far from the point below the light. The hull is along Y from the
//! nose at y = 1.5 to the tail at -1.25 (model units -3 to 2.5 at 0.5 a unit), at
//! height -1, so its shadow runs from y = 2.6 to -2.2 on the line x = 0. The wings are
//! swept back from x = 0.3 to 1.5 and y = -0.1 to -1.6, so that the middle of the
//! right wing, at (0.9, -0.9), throws its shadow to (1.56, -1.56).
//!
//! A machine with no GPU skips this, unless `MSCENT_REQUIRE_GPU` is set.

#[path = "support/gpu.rs"]
mod gpu_support;
#[allow(dead_code)]
#[path = "support/shade.rs"]
mod shade;

use mscent_math::Vec3;
use mscent_render::View;
use mscent_render::model::ModelInstance;
use shade::{
    Contents, FLOOR_Z, OFF, ON, Room, SIZE, Shot, box_at, keep, lamp, level_at, pixel_of, room,
    ship_at, view,
};

fn v(x: f32, y: f32, z: f32) -> Vec3 {
    Vec3::new(x, y, z)
}

const SHIP_AT: Vec3 = Vec3::new(0.0, 0.0, -1.0);
const SCALE: f32 = 2.0;

fn setup() -> Option<(Shot, [mscent_render::DynLight; 1])> {
    let shot = Shot::new(&gpu_support::gpu()?, room(Room::default()));
    let light = [lamp(&shot.level, v(0.0, 0.0, 3.0))];
    Some((shot, light))
}

fn level_of(shot: &Shot, frame: &[u32], x: f32, y: f32) -> f32 {
    let cam = shot.camera(&view());
    level_at(frame, pixel_of(&cam, v(x, y, FLOOR_Z)))
}

/// The frame with the ship in it as first person has it, shadows on and off.
fn frames(
    shot: &mut Shot,
    lights: &[mscent_render::DynLight],
    ships: &[ModelInstance],
) -> [Vec<u32>; 2] {
    let contents = Contents {
        lights,
        boxes: ships,
        ..Contents::default()
    };
    [OFF, ON].map(|effects| shot.frame(&view(), effects, &contents))
}

#[test]
fn the_ship_in_first_person_casts_its_shadow_where_by_hand_it_should() {
    let Some((mut shot, lights)) = setup() else {
        return;
    };
    let ships = [ship_at(SHIP_AT, 0.0, SCALE, true)];
    let [off, on] = frames(&mut shot, &lights, &ships);
    keep("ship_on.png", &on);
    for (x, y, what) in [
        (0.0, 0.0, "the hull's middle"),
        (0.0, 1.5, "toward the nose"),
        (0.0, -1.4, "toward the tail"),
        (1.56, -1.56, "the right wing"),
        (-1.56, -1.56, "the left wing"),
    ] {
        let (lit, shaded) = (level_of(&shot, &off, x, y), level_of(&shot, &on, x, y));
        assert!(lit > 40.0, "{what} is lit without shadows: {lit}");
        assert!(
            shaded < 0.1 * lit,
            "{what} is in the shadow: {shaded} of {lit}"
        );
    }
    for (x, y, what) in [
        (3.5, 0.0, "beside the hull"),
        (0.0, -4.8, "past the tail"),
        (-3.5, -1.5, "past the left wing"),
        (1.6, 1.0, "between the nose and the wing"),
    ] {
        let (lit, shaded) = (level_of(&shot, &off, x, y), level_of(&shot, &on, x, y));
        assert!(
            (shaded - lit).abs() <= 0.03 * lit + 1.0,
            "{what} is out of it: {shaded} of {lit}"
        );
    }
}

/// A floor point's level without shadows and with them, for a ship in the light.
fn pair(
    shot: &mut Shot,
    lights: &[mscent_render::DynLight],
    ship: ModelInstance,
    at: (f32, f32),
) -> (f32, f32) {
    let [off, on] = frames(shot, lights, &[ship]);
    (
        level_of(shot, &off, at.0, at.1),
        level_of(shot, &on, at.0, at.1),
    )
}

#[test]
fn the_shadow_follows_the_ships_position_and_heading() {
    let Some((mut shot, lights)) = setup() else {
        return;
    };
    let home = ship_at(SHIP_AT, 0.0, SCALE, true);
    let (lit, shaded) = pair(&mut shot, &lights, home, (0.0, 0.0));
    assert!(shaded < 0.1 * lit, "at home: {shaded} of {lit}");
    // Moved 3 along +X, its shadow goes 7 / 4 as far from under the light: to x = 5.2.
    let moved = ship_at(SHIP_AT + v(3.0, 0.0, 0.0), 0.0, SCALE, true);
    let (lit, shaded) = pair(&mut shot, &lights, moved, (5.2, 0.0));
    assert!(
        shaded < 0.1 * lit,
        "moved, under the new shadow: {shaded} of {lit}"
    );
    let (lit, shaded) = pair(&mut shot, &lights, moved, (0.0, 0.0));
    assert!(
        (shaded - lit).abs() <= 0.03 * lit + 1.0,
        "moved, the old place: {shaded} of {lit}"
    );
    // Facing 90 degrees left, the hull lies along X: dark at (1.5, 0), lit at (0, 1.5).
    let turned = ship_at(SHIP_AT, std::f32::consts::FRAC_PI_2, SCALE, true);
    let (lit, shaded) = pair(&mut shot, &lights, turned, (1.5, 0.0));
    assert!(shaded < 0.1 * lit, "turned, along X: {shaded} of {lit}");
    let (lit, shaded) = pair(&mut shot, &lights, turned, (0.0, 1.5));
    assert!(
        (shaded - lit).abs() <= 0.03 * lit + 1.0,
        "turned, where the nose was: {shaded} of {lit}"
    );
}

#[test]
fn the_ship_is_not_in_a_first_person_frame_and_is_in_a_chase_one() {
    let Some((mut shot, lights)) = setup() else {
        return;
    };
    let with = |cast_only| [ship_at(SHIP_AT, 0.0, SCALE, cast_only)];
    let gone = frames(&mut shot, &lights, &[]);
    let hidden = frames(&mut shot, &lights, &with(true));
    assert_eq!(
        hidden[0], gone[0],
        "shadows off: the frame without the ship"
    );
    // With them on, only the shadow differs, so every pixel the ship would cover
    // is the plain floor or wall: the ship's own pixels never appear. Seen as a
    // test of absence: no pixel above the hull's height differs.
    let drawn = frames(&mut shot, &lights, &with(false));
    keep("ship_chase.png", &drawn[0]);
    let changed = drawn[0]
        .iter()
        .zip(&gone[0])
        .filter(|(a, b)| a != b)
        .count();
    assert!(changed > 30, "drawn, the ship covers {changed} pixels");
    let cam = shot.camera(&view());
    let (x, y) = pixel_of(&cam, SHIP_AT);
    assert_ne!(
        drawn[0][y * SIZE.0 as usize + x],
        gone[0][y * SIZE.0 as usize + x]
    );
    assert_eq!(
        hidden[0][y * SIZE.0 as usize + x],
        gone[0][y * SIZE.0 as usize + x]
    );
}

#[test]
fn a_model_the_camera_does_not_see_still_casts() {
    let Some((mut shot, _)) = setup() else {
        return;
    };
    // The camera stands at (0, -5, 3) looking steeply down; the box is 1.5 behind it
    // and 4 below, out of the frustum, and the light is behind the box, so that the
    // box's shadow falls ahead of the camera, at (0, -1.5) on the floor.
    let eye = View::from_angles(v(0.0, -5.0, 3.0), 0.0, -1.0, 0.0);
    let light = [lamp(&shot.level, v(0.0, -9.0, 1.0))];
    let boxes = [box_at(v(0.0, -6.5, -1.0))];
    let contents = Contents {
        lights: &light,
        boxes: &boxes,
        ..Contents::default()
    };
    let (off, on) = (
        shot.frame(&eye, OFF, &contents),
        shot.frame(&eye, ON, &contents),
    );
    let without = shot.frame(
        &eye,
        OFF,
        &Contents {
            lights: &light,
            ..Contents::default()
        },
    );
    assert_eq!(off, without, "the box is out of the camera's view");
    keep("unseen_on.png", &on);
    let cam = shot.camera(&eye);
    let at = |frame: &[u32], p: Vec3| level_at(frame, pixel_of(&cam, p));
    let p = v(0.0, -1.5, FLOOR_Z);
    assert!(
        at(&on, p) < 0.1 * at(&off, p),
        "its shadow: {} of {}",
        at(&on, p),
        at(&off, p)
    );
}

#[test]
fn hostile_ship_poses_do_not_panic() {
    let Some((mut shot, lights)) = setup() else {
        return;
    };
    let nan = f32::NAN;
    let base = ship_at(SHIP_AT, 0.0, SCALE, true);
    let zero = mscent_math::Quat {
        x: 0.0,
        y: 0.0,
        z: 0.0,
        w: 0.0,
    };
    let bad = [
        ModelInstance {
            pos: v(nan, 0.0, 0.0),
            ..base
        },
        ModelInstance {
            pos: v(1.0e30, 0.0, 0.0),
            ..base
        },
        ModelInstance {
            pos: v(f32::INFINITY, 0.0, -1.0),
            ..base
        },
        ModelInstance { rot: zero, ..base },
        ModelInstance {
            rot: mscent_math::Quat { x: nan, ..base.rot },
            ..base
        },
        ModelInstance { scale: nan, ..base },
        ModelInstance { scale: 0.0, ..base },
        ModelInstance {
            scale: 1.0e9,
            ..base
        },
        ModelInstance {
            scale: -3.0,
            ..base
        },
        ModelInstance {
            tint: v(nan, 1.0, 1.0),
            ..base
        },
    ];
    let plain = shot.frame(
        &view(),
        ON,
        &Contents {
            lights: &lights,
            ..Contents::default()
        },
    );
    for instance in bad {
        for effects in [OFF, ON] {
            let contents = Contents {
                lights: &lights,
                boxes: std::slice::from_ref(&instance),
                ..Contents::default()
            };
            // Every frame comes out, with no GPU error (`Shot::frame` asserts it).
            let frame = shot.frame(&view(), effects, &contents);
            // A ship whose pose cannot be used leaves the frame as it was.
            let unusable = instance.pos.x.is_nan() || instance.scale <= 0.0 || instance.rot == zero;
            if unusable && effects == ON {
                assert_eq!(frame, plain, "{instance:?}");
            }
        }
    }
}
