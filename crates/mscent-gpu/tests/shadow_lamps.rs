//! Shadows from the level's static lamps, read from real pixels. A lamp's light is
//! baked into the lightmap with the level standing still; what moves shades it
//! through the maps in the frame, by taking the lamp's baked share from the faces
//! the mover's shadow falls on (`docs/hardware-effects.md`).
//!
//! The room is the one of `shadows.rs`, with a baked lamp where its dynamic light
//! was: 3 above the middle of a 2 by 2 by 2 box, so the shadow of the box's top
//! falls on the floor 7 below the lamp with a half width of 7 / 3.
//!
//! A machine with no GPU skips this, unless `MSCENT_REQUIRE_GPU` is set.

#[path = "support/gpu.rs"]
mod gpu_support;
#[allow(dead_code)]
#[path = "support/shade.rs"]
mod shade;

use mscent_gpu::Gpu;
use mscent_math::Vec3;
use mscent_render::{Actor, ActorKind, MoverPose};
use shade::{Contents, OFF, ON, Room, Shot, box_at, keep, level_at, pixel_of, room, ship_at, view};

fn v(x: f32, y: f32, z: f32) -> Vec3 {
    Vec3::new(x, y, z)
}

const SHADOW_HALF: f32 = 7.0 / 3.0;
const FLOOR: f32 = shade::FLOOR_Z;

/// A room lit by one baked lamp over the middle.
fn lit_room() -> Room {
    Room {
        baked_lamp: Some((v(0.0, 0.0, 3.0), 1.0)),
        ..Room::default()
    }
}

/// The same room, the lamp a projector aimed at the floor with a cone wide enough
/// to hold the box's whole shadow in full light.
fn projector_room() -> Room {
    Room {
        cone: Some(0.9),
        ..lit_room()
    }
}

fn shot(spec: Room) -> Shot {
    Shot::new(&gpu_support::gpu().expect("a gpu"), room(spec))
}

fn floor_level(shot: &Shot, frame: &[u32], x: f32, y: f32) -> f32 {
    let cam = shot.camera(&view());
    level_at(frame, pixel_of(&cam, v(x, y, FLOOR)))
}

fn changed(a: &[u32], b: &[u32]) -> usize {
    a.iter().zip(b).filter(|(p, q)| p != q).count()
}

macro_rules! with_gpu {
    () => {
        if gpu_support::gpu().is_none() {
            return;
        }
    };
}

#[test]
fn a_box_shades_the_floor_under_a_baked_lamp_and_not_beside_it() {
    with_gpu!();
    let mut shot = shot(lit_room());
    let boxes = [box_at(v(0.0, 0.0, -1.0))];
    let contents = Contents {
        boxes: &boxes,
        ..Contents::default()
    };
    let (off, on) = (
        shot.frame(&view(), OFF, &contents),
        shot.frame(&view(), ON, &contents),
    );
    keep("lamp_box_off.png", &off);
    keep("lamp_box_on.png", &on);
    let (inside, outside) = (SHADOW_HALF - 0.9, SHADOW_HALF + 0.9);
    for (x, y) in [(-inside, -inside), (inside, -inside), (0.0, -inside - 0.2)] {
        let (lit, shaded) = (
            floor_level(&shot, &off, x, y),
            floor_level(&shot, &on, x, y),
        );
        assert!(lit > 40.0, "({x}, {y}) is lit by the lamp: {lit}");
        // The bake's ambient is left: the lamp's share is what the box takes.
        assert!(
            shaded < 0.5 * lit,
            "({x}, {y}) is in the shadow: {shaded} of {lit}"
        );
    }
    for (x, y) in [(-outside, -inside), (outside, -inside), (0.0, -outside)] {
        let (lit, shaded) = (
            floor_level(&shot, &off, x, y),
            floor_level(&shot, &on, x, y),
        );
        assert!(
            (shaded - lit).abs() <= 0.03 * lit + 1.0,
            "({x}, {y}) is beside the shadow: {shaded} of {lit}"
        );
    }
}

#[test]
fn a_box_shades_the_floor_under_a_projector_and_not_beside_its_shadow() {
    with_gpu!();
    let mut shot = shot(projector_room());
    let boxes = [box_at(v(0.0, 0.0, -1.0))];
    let contents = Contents {
        boxes: &boxes,
        ..Contents::default()
    };
    let (off, on) = (
        shot.frame(&view(), OFF, &contents),
        shot.frame(&view(), ON, &contents),
    );
    keep("projector_box_off.png", &off);
    keep("projector_box_on.png", &on);
    let (inside, outside) = (SHADOW_HALF - 0.9, SHADOW_HALF + 0.9);
    for (x, y) in [(-inside, -inside), (inside, -inside), (0.0, -inside - 0.2)] {
        let (lit, shaded) = (
            floor_level(&shot, &off, x, y),
            floor_level(&shot, &on, x, y),
        );
        assert!(lit > 40.0, "({x}, {y}) is lit by the projector: {lit}");
        assert!(
            shaded < 0.5 * lit,
            "({x}, {y}) is in the shadow: {shaded} of {lit}"
        );
    }
    for (x, y) in [(-outside, -inside), (outside, -inside), (0.0, -outside)] {
        let (lit, shaded) = (
            floor_level(&shot, &off, x, y),
            floor_level(&shot, &on, x, y),
        );
        assert!(
            (shaded - lit).abs() <= 0.03 * lit + 1.0,
            "({x}, {y}) is beside the shadow: {shaded} of {lit}"
        );
    }
}

#[test]
fn a_projector_aimed_away_from_a_box_leaves_its_floor_as_it_was() {
    with_gpu!();
    // The cone is 0.2 wide about straight down: the box's shadow, 7/3 to the side
    // on a floor 5 below, is far outside it, and nothing there was lit to take from.
    let mut shot = shot(Room {
        cone: Some(0.2),
        ..lit_room()
    });
    let boxes = [box_at(v(0.0, 0.0, -1.0))];
    let contents = Contents {
        boxes: &boxes,
        ..Contents::default()
    };
    let (off, on) = (
        shot.frame(&view(), OFF, &contents),
        shot.frame(&view(), ON, &contents),
    );
    for (x, y) in [(-2.0, -2.0), (2.0, -2.0), (0.0, -3.0), (4.0, 4.0)] {
        let (before, after) = (
            floor_level(&shot, &off, x, y),
            floor_level(&shot, &on, x, y),
        );
        assert!(
            (after - before).abs() <= 1.0,
            "({x}, {y}) is outside the cone: {after} of {before}"
        );
    }
}

#[test]
fn with_nothing_that_moves_the_frame_is_the_frame_without_shadows() {
    with_gpu!();
    let mut shot = shot(lit_room());
    let none = Contents::default();
    let off = shot.frame(&view(), OFF, &none);
    assert_eq!(shot.frame(&view(), ON, &none), off);
}

#[test]
fn a_shaded_floor_is_never_darker_than_nothing_and_never_lighter_than_it_was() {
    with_gpu!();
    // A pillar between the lamp and the box leaves the floor dark there in the
    // bake; the box's shadow must not take light that is not there.
    let spec = Room {
        pillar: true,
        baked_lamp: Some((v(5.0, 0.0, 3.0), 1.0)),
        ..Room::default()
    };
    let mut shot = shot(spec);
    let boxes = [box_at(v(-4.0, 0.0, -1.0))];
    let contents = Contents {
        boxes: &boxes,
        ..Contents::default()
    };
    let (off, on) = (
        shot.frame(&view(), OFF, &contents),
        shot.frame(&view(), ON, &contents),
    );
    keep("lamp_pillar_on.png", &on);
    for (&lit, &shaded) in off.iter().zip(&on) {
        let level = |c: u32| (c >> 16 & 255) + (c >> 8 & 255) + (c & 255);
        assert!(
            level(shaded) <= level(lit) + 3,
            "a shadow only darkens: {shaded:08x} of {lit:08x}"
        );
    }
    assert!(changed(&off, &on) > 0, "and the box does shade something");
}

#[test]
fn a_ship_an_actor_and_a_mover_all_shade_the_floor_under_a_baked_lamp() {
    with_gpu!();
    let spec = Room {
        slab: true,
        ..lit_room()
    };
    let mut shot = shot(spec);
    let moved = [MoverPose {
        offset: v(4.0, 0.0, 0.0),
        ..MoverPose::AUTHORED
    }];
    let actors = [Actor {
        kind: ActorKind::Crate,
        pos: v(-4.0, 0.0, FLOOR + 1.0),
        yaw: 0.0,
    }];
    let ships = [ship_at(v(0.0, 4.0, -1.0), 0.0, 1.0, false)];
    let contents = Contents {
        actors: &actors,
        poses: &moved,
        boxes: &ships,
        ..Contents::default()
    };
    let (off, on) = (
        shot.frame(&view(), OFF, &contents),
        shot.frame(&view(), ON, &contents),
    );
    keep("lamp_all_off.png", &off);
    keep("lamp_all_on.png", &on);
    assert!(changed(&off, &on) > 300, "{}", changed(&off, &on));
    // The mover moved by 4 casts a shadow from 7 to 11.7 along the floor.
    let (slab, before) = (
        floor_level(&shot, &on, 9.0, -1.0),
        floor_level(&shot, &off, 9.0, -1.0),
    );
    assert!(
        slab < 0.5 * before,
        "the mover's shadow: {slab} of {before}"
    );
    // The crate, 1 above the floor under a lamp 7 above it, is shadowed from the
    // lamp past it, 1.2 times as far out along the floor.
    // Its size is not known by hand, so the darkest of a grid round that point.
    let grid =
        (0..8).flat_map(|i| (0..5).map(move |j| (-5.4 + 0.2 * i as f32, -0.4 + 0.2 * j as f32)));
    let (crate_, before) = grid
        .map(|(x, y)| {
            (
                floor_level(&shot, &on, x, y),
                floor_level(&shot, &off, x, y),
            )
        })
        .min_by(|a, b| (a.0 / a.1).total_cmp(&(b.0 / b.1)))
        .expect("a grid");
    assert!(
        crate_ < 0.6 * before,
        "the actor's shadow: {crate_} of {before}"
    );
}

#[test]
fn a_dynamic_light_and_a_baked_lamp_each_shade_their_own_light() {
    with_gpu!();
    let mut shot = shot(lit_room());
    let lights = [shade::lamp(&shot.level, v(0.0, 0.0, 3.0))];
    let boxes = [box_at(v(0.0, 0.0, -1.0))];
    let contents = Contents {
        lights: &lights,
        boxes: &boxes,
        ..Contents::default()
    };
    let (off, on) = (
        shot.frame(&view(), OFF, &contents),
        shot.frame(&view(), ON, &contents),
    );
    let (x, y) = (-1.2, -1.2);
    let (lit, shaded) = (
        floor_level(&shot, &off, x, y),
        floor_level(&shot, &on, x, y),
    );
    // Both lights are blocked: little but the ambient is left.
    assert!(shaded < 0.25 * lit, "{shaded} of {lit}");
}

/// The lamp room with a box under the lamp, with shadows on twice and off once.
fn frames(gpu: &Gpu) -> (Vec<u32>, Vec<u32>) {
    let mut shot = Shot::new(gpu, room(lit_room()));
    let boxes = [box_at(v(0.0, 0.0, -1.0))];
    let contents = Contents {
        boxes: &boxes,
        ..Contents::default()
    };
    let on = shot.frame(&view(), ON, &contents);
    assert_eq!(shot.frame(&view(), ON, &contents), on, "steady");
    (on, shot.frame(&view(), OFF, &contents))
}

fn small(cap: impl FnOnce(wgpu::Limits) -> wgpu::Limits) -> Option<Gpu> {
    gpu_support::gpu()?;
    Some(Gpu::with_limits(cap).expect("lower limits are always allowed"))
}

#[test]
fn a_gpu_without_room_for_the_maps_draws_the_lamps_floor_unshaded() {
    // Layers for the dynamic lights' maps, not for the lamps' as well.
    let Some(gpu) = small(|o| wgpu::Limits {
        max_texture_array_layers: 12,
        ..o
    }) else {
        return;
    };
    let (on, off) = frames(&gpu);
    assert_eq!(on, off);
    assert_eq!(gpu.take_error(), None);
}

#[test]
fn a_gpu_without_cube_arrays_draws_the_lamps_floor_unshaded() {
    let Some(gpu) = small(|o| wgpu::Limits {
        max_texture_array_layers: 4,
        ..o
    }) else {
        return;
    };
    let (on, off) = frames(&gpu);
    assert_eq!(on, off);
    assert_eq!(gpu.take_error(), None);
}

/// The control: the real GPU does shade them.
#[test]
fn the_same_scene_on_the_real_gpu_is_shaded() {
    let Some(gpu) = gpu_support::gpu() else {
        return;
    };
    let (on, off) = frames(&gpu);
    assert_ne!(on, off);
}
