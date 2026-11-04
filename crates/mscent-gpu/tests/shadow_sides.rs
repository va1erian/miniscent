//! A shadow falls right for a light on every side of the thing that casts it, which
//! reads all six faces of the light's cube and their orientations. Each case is a
//! light, a view, and the floor or ceiling point where the ray from the light through
//! the box's centre lands, worked by hand: it is `light + t * (centre - light)` where
//! that meets the plane. That point reads under a tenth of its unshadowed light, and
//! one 4.5 along from it reads as it did.
//!
//! A machine with no GPU skips this, unless `MSCENT_REQUIRE_GPU` is set.

#[path = "support/gpu.rs"]
mod gpu_support;
#[allow(dead_code)]
#[path = "support/shade.rs"]
mod shade;

use mscent_math::Vec3;
use mscent_render::{DynLight, View};
use shade::{Contents, FLOOR_Z, OFF, ON, Room, Shot, box_at, keep, level_at, pixel_of, room, view};

fn v(x: f32, y: f32, z: f32) -> Vec3 {
    Vec3::new(x, y, z)
}

struct Side {
    name: &'static str,
    light: Vec3,
    view: View,
    /// The box's scale: the one below the light is small, so that its shadow on the
    /// ceiling stays on it.
    scale: f32,
    shaded: Vec3,
    lit: Vec3,
}

/// A light on each side of the box, and one below it. The box is centred at
/// (0, 0, -1). A light 6 to one side and 3 up has its ray meet the floor, 3 below
/// the centre, at t = 7 / 4 of the way, which is 4.5 on the other side.
fn sides() -> [Side; 5] {
    let from_south = view();
    let from_north = View::from_angles(v(0.0, 10.5, 2.5), std::f32::consts::PI, -0.5, 0.0);
    // Below the box a light's ray to its centre goes up by (-2, 0, 2.5): it meets the
    // ceiling at 8 where t = 11.5 / 2.5 = 4.6, so x = 2 - 9.2. The view has the box in
    // it as well as the ceiling, as only models the camera sees cast.
    let from_below = View::from_angles(v(-2.5, -7.0, -1.5), 0.125, 0.46, 0.0);
    let case = |name, light, view: &View, scale, shaded, lit| Side {
        name,
        light,
        view: *view,
        scale,
        shaded,
        lit,
    };
    [
        case(
            "+x",
            v(6.0, 0.0, 3.0),
            &from_south,
            1.0,
            v(-4.5, 0.0, FLOOR_Z),
            v(-4.5, 4.5, FLOOR_Z),
        ),
        case(
            "-x",
            v(-6.0, 0.0, 3.0),
            &from_south,
            1.0,
            v(4.5, 0.0, FLOOR_Z),
            v(4.5, 4.5, FLOOR_Z),
        ),
        case(
            "+y",
            v(0.0, 6.0, 3.0),
            &from_south,
            1.0,
            v(0.0, -4.5, FLOOR_Z),
            v(4.5, -4.5, FLOOR_Z),
        ),
        case(
            "-y",
            v(0.0, -6.0, 3.0),
            &from_north,
            1.0,
            v(0.0, 4.5, FLOOR_Z),
            v(4.5, 4.5, FLOOR_Z),
        ),
        case(
            "+z",
            v(2.0, 0.0, -3.5),
            &from_below,
            0.4,
            v(-7.2, 0.0, 8.0),
            v(-7.2, 4.5, 8.0),
        ),
    ]
}

#[test]
fn a_shadow_falls_right_for_a_light_on_every_side_of_the_box() {
    let Some(gpu) = gpu_support::gpu() else {
        return;
    };
    let mut shot = Shot::new(&gpu, room(Room::default()));
    for side in sides() {
        let mut light = DynLight::new();
        assert!(light.place(shot.level.scene(), side.light, 30.0, v(3.0, 3.0, 3.0)));
        let lights = [light];
        let mut instance = box_at(v(0.0, 0.0, -1.0));
        instance.scale = side.scale;
        let boxes = [instance];
        let contents = Contents {
            lights: &lights,
            boxes: &boxes,
            ..Contents::default()
        };
        let (off, on) = (
            shot.frame(&side.view, OFF, &contents),
            shot.frame(&side.view, ON, &contents),
        );
        keep(&format!("side_{}.png", side.name), &on);
        let cam = shot.camera(&side.view);
        let at = |frame: &[u32], p: Vec3| level_at(frame, pixel_of(&cam, p));
        let (lit, shaded) = (at(&off, side.shaded), at(&on, side.shaded));
        assert!(lit > 30.0, "{}: lit without shadows: {lit}", side.name);
        assert!(
            shaded < 0.1 * lit,
            "{}: in the shadow: {shaded} of {lit}",
            side.name
        );
        let (beside_lit, beside) = (at(&off, side.lit), at(&on, side.lit));
        assert!(
            beside_lit > 10.0 && (beside - beside_lit).abs() <= 0.03 * beside_lit + 1.0,
            "{}: beside the shadow: {beside} of {beside_lit}",
            side.name
        );
    }
}
