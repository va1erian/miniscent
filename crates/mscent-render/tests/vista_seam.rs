//! A miniature that continues the play space is lit by the play space's lamps,
//! so the two meet at the same brightness: judged on baked luxels at twin
//! points, one in the play room and its image in the layer's room.

#[allow(dead_code)]
#[path = "support/vista_room.rs"]
mod room;

use mscent_math::Vec3;
use mscent_world::Entity;
use mscent_world::Op;
use mscent_world::materials::METAL;
use room::*;

fn near_lamp() -> Entity {
    lamp(Vec3::new(0.0, 40.0, 0.0))
}

#[test]
fn the_floor_beyond_a_viewport_is_lit_as_its_twin_in_the_play_space() {
    let r = renderer(Vec::new(), vec![near_lamp(), layer(PARALLAX)]);
    for y in [24.0, 32.0, 40.0, 44.0, 47.0] {
        for x in [-8.0, 0.0, 6.0] {
            let p = Vec3::new(x, y, FLOOR);
            let (play, view) = (floor_light(&r, p), floor_light(&r, image(p)));
            let (a, b) = (luminance(play), luminance(view));
            println!("({x}, {y}): play {a:.4}, vista {b:.4}");
            assert!(a > 0.05, "the lamp lights the play floor at ({x}, {y})");
            assert!((a - b).abs() <= 0.12 * a, "({x}, {y}): {a} against {b}");
        }
    }
}

#[test]
fn the_floor_past_the_viewport_wall_takes_the_lamps_light() {
    let lit = renderer(Vec::new(), vec![near_lamp(), layer(PARALLAX)]);
    let dark = renderer(Vec::new(), vec![layer(PARALLAX)]);
    // Past the wall at y = 48: only the image of the lamp can light it.
    for y in [49.0, 52.0, 58.0] {
        let p = image(Vec3::new(0.0, y, FLOOR));
        let (a, b) = (
            luminance(floor_light(&lit, p)),
            luminance(floor_light(&dark, p)),
        );
        println!("y {y}: with the lamp {a:.4}, without {b:.4}");
        assert!(a > b * 3.0, "y {y}: {a} against {b}");
    }
}

#[test]
fn a_lamp_behind_the_viewport_wall_lights_nothing_of_the_vista() {
    // A room behind the viewport, with a lamp whose image would fall in the view.
    let behind = cuboid(
        [-16.0, 60.0, FLOOR],
        [16.0, 90.0, 16.0],
        Op::Subtract,
        3,
        METAL,
    );
    let back_lamp = lamp(Vec3::new(0.0, 64.0, 0.0));
    let with = renderer(vec![behind.clone()], vec![back_lamp, layer(PARALLAX)]);
    let without = renderer(vec![behind], vec![layer(PARALLAX)]);
    for y in [50.0, 60.0, 70.0] {
        let p = image(Vec3::new(0.0, y, FLOOR));
        assert_eq!(floor_light(&with, p), floor_light(&without, p), "y {y}");
    }
}

#[test]
fn an_infinitely_far_layer_takes_no_play_lamp() {
    let with = renderer(Vec::new(), vec![near_lamp(), layer(0.0)]);
    let without = renderer(Vec::new(), vec![layer(0.0)]);
    for y in [40.0, 52.0] {
        let p = image(Vec3::new(0.0, y, FLOOR));
        assert_eq!(floor_light(&with, p), floor_light(&without, p), "y {y}");
    }
}

#[test]
fn a_room_shown_by_an_infinite_layer_first_is_lit_by_the_finite_one() {
    // The first layer cannot carry a lamp; the room still takes the second's.
    let shown = vec![layer_at(0.0, 0), layer_at(PARALLAX, 1)];
    let lit = renderer(Vec::new(), [vec![near_lamp()], shown.clone()].concat());
    let dark = renderer(Vec::new(), shown);
    let p = image(Vec3::new(0.0, 52.0, FLOOR));
    let (a, b) = (
        luminance(floor_light(&lit, p)),
        luminance(floor_light(&dark, p)),
    );
    assert!(a > b * 3.0, "{a} against {b}");
}
