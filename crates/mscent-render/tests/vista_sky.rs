//! Sky light: a lamp in a layer's room lights the play space through the
//! viewport, as a Source level is lit through its sky. Judged on baked luxels.

#[allow(dead_code)]
#[path = "support/vista_room.rs"]
mod room;

use mscent_math::Vec3;
use mscent_world::materials::METAL;
use mscent_world::{Entity, Op};
use room::*;

/// The sun, in play-space terms: high beyond the viewport wall. Its lamp stands
/// in the layer's room at this point's image, 4 times smaller.
const SUN: Vec3 = Vec3::new(0.0, 100.0, 8.0);
const SUN_RADIUS: f32 = 120.0;

fn sun() -> Entity {
    lamp_of(image(SUN), SUN_RADIUS * PARALLAX)
}

/// Floor points the sun sees through the viewport, in the play space.
fn floor_points() -> Vec<Vec3> {
    let ys = [20.0, 30.0, 40.0];
    let xs = [-8.0, 0.0, 8.0];
    ys.iter()
        .flat_map(|&y| xs.iter().map(move |&x| Vec3::new(x, y, FLOOR)))
        .collect()
}

fn lum(r: &mscent_render::LevelRenderer, p: Vec3) -> f32 {
    luminance(floor_light(r, p))
}

#[test]
fn the_room_sun_lights_the_play_floor_as_it_lights_the_floors_image() {
    let r = renderer(Vec::new(), vec![sun(), layer(PARALLAX)]);
    let dark = renderer(Vec::new(), vec![layer(PARALLAX)]);
    for p in floor_points() {
        let (play, view, none) = (lum(&r, p), lum(&r, image(p)), lum(&dark, p));
        println!("{p:?}: play {play:.4}, its image {view:.4}, no sun {none:.4}");
        assert!(play > none * 3.0, "{p:?}: the sun reaches the play floor");
        assert!(
            (play - view).abs() <= 0.15 * view,
            "{p:?}: {play} against {view}"
        );
    }
}

#[test]
fn rock_in_the_room_shades_the_play_space() {
    // A wall across the room between the sun and the viewport's image, with a
    // gap over it so that the room stays one room and the sun stays in it.
    let lo = image(Vec3::new(-48.0, 60.0, FLOOR));
    let hi = image(Vec3::new(48.0, 64.0, 24.0));
    let rock = || cuboid(arr(lo), arr(hi), Op::Add, 3, METAL);
    let shaded = renderer(vec![rock()], vec![sun(), layer(PARALLAX)]);
    let dark = renderer(vec![rock()], vec![layer(PARALLAX)]);
    // The same sun with the rock moved past it, where it shades nothing: lit, so
    // the darkness above is the rock's and not a missing sun.
    let behind = |y: f32| image(Vec3::new(0.0, y, FLOOR));
    let (blo, bhi) = (behind(150.0), image(Vec3::new(48.0, 154.0, 24.0)));
    let past = cuboid(
        arr(Vec3::new(lo.x, blo.y, lo.z)),
        arr(bhi),
        Op::Add,
        3,
        METAL,
    );
    let open = renderer(vec![past], vec![sun(), layer(PARALLAX)]);
    for p in floor_points() {
        let (a, b, c) = (lum(&shaded, p), lum(&dark, p), lum(&open, p));
        assert!((a - b).abs() <= 0.01, "{p:?}: {a} against {b}");
        assert!(
            c > b * 3.0,
            "{p:?}: the sun reaches it past the rock, {c} against {b}"
        );
    }
}

#[test]
fn a_wall_of_the_play_space_shades_its_floor_from_the_sun() {
    // Half height, across the room in front of the floor at y 30: the sun's rays
    // to it pass under the top of the wall.
    let wall = || cuboid([-16.0, 36.0, FLOOR], [16.0, 38.0, 0.0], Op::Add, 3, METAL);
    let lit = renderer(vec![wall()], vec![sun(), layer(PARALLAX)]);
    let dark = renderer(vec![wall()], vec![layer(PARALLAX)]);
    let p = Vec3::new(0.0, 30.0, FLOOR);
    let (a, b) = (lum(&lit, p), lum(&dark, p));
    assert!((a - b).abs() <= 0.01, "{a} against {b}");
    // Past the wall, the same sun still lands.
    let q = Vec3::new(0.0, 44.0, FLOOR);
    assert!(lum(&lit, q) > lum(&dark, q) * 3.0);
}

#[test]
fn an_infinitely_far_layer_sheds_no_sky_light() {
    let with = renderer(Vec::new(), vec![sun(), layer(0.0)]);
    let without = renderer(Vec::new(), vec![layer(0.0)]);
    for p in floor_points() {
        assert_eq!(floor_light(&with, p), floor_light(&without, p), "{p:?}");
    }
}
