//! Shadows add nothing where nothing is shadowed, and switching them off draws the
//! frame the renderer draws without them. Each is proved the way that holds on any
//! adapter and driver: frames of one process are compared with each other, never
//! with pixels remembered from another machine.
//!
//! * shadows on with no light, and with lights but nothing that casts, draw what
//!   shadows off draw;
//! * shadows off, in a renderer made apart from the first, draw the same frame
//!   as both looks off (parallax needs a height map, and this room's materials
//!   have none), so the switch alone is what is off;
//! * a light that is there changes the frame, so the equalities are not of blanks.
//!
//! A frame from before the shadows existed (commit b3c0e6d) hashed 0x4d4c_1f56_c230_8935
//! on the machine that took it. That is printed for a person to compare and does
//! not fail anything: other drivers round differently.
//!
//! A machine with no GPU skips this, unless `MSCENT_REQUIRE_GPU` is set.

#[path = "support/gpu.rs"]
mod gpu_support;
#[allow(dead_code)]
#[path = "support/shade.rs"]
mod shade;

use mscent_gpu::GpuEffects;
use mscent_math::Vec3;
use shade::{Contents, OFF, ON, Room, Shot, box_at, fnv, lamp, room, view};

fn v(x: f32, y: f32, z: f32) -> Vec3 {
    Vec3::new(x, y, z)
}

fn spec() -> Room {
    Room {
        baked_lamp: Some((v(6.0, 6.0, 5.0), 2.0)),
        pillar: true,
        ..Room::default()
    }
}

#[test]
fn shadows_that_shade_nothing_draw_the_frame_without_them() {
    let Some(gpu) = gpu_support::gpu() else {
        return;
    };
    let mut shot = Shot::new(&gpu, room(spec()));
    let lights = [
        lamp(&shot.level, v(0.0, -4.0, 3.0)),
        lamp(&shot.level, v(5.0, 2.0, 1.0)),
    ];
    let boxes = [box_at(v(-3.0, -5.0, -1.0)), box_at(v(3.0, -6.0, -2.0))];
    let lit = Contents {
        lights: &lights,
        boxes: &boxes,
        ..Contents::default()
    };
    let no_casters = Contents {
        lights: &lights,
        ..Contents::default()
    };
    let nothing = Contents::default();

    let off = shot.frame(&view(), OFF, &lit);
    shade::keep("off.png", &off);
    eprintln!("informational: the lit room hashes {:#018x}", fnv(&off));
    let bare = shot.frame(&view(), OFF, &nothing);
    assert_ne!(off, bare, "the lights light the room");

    assert_eq!(
        shot.frame(&view(), ON, &nothing),
        bare,
        "shadows on, no light"
    );
    assert_eq!(
        shot.frame(&view(), ON, &no_casters),
        shot.frame(&view(), OFF, &no_casters),
        "shadows on, lights and nothing in their way"
    );
    assert_ne!(
        shot.frame(&view(), ON, &lit),
        off,
        "with something in the way they do shade"
    );
}

#[test]
fn the_switch_alone_is_what_turns_shadows_off() {
    let Some(gpu) = gpu_support::gpu() else {
        return;
    };
    let mut first = Shot::new(&gpu, room(spec()));
    let mut second = Shot::new(&gpu, room(spec()));
    let lights = [lamp(&first.level, v(0.0, -4.0, 3.0))];
    let boxes = [box_at(v(-3.0, -5.0, -1.0))];
    let contents = Contents {
        lights: &lights,
        boxes: &boxes,
        ..Contents::default()
    };
    let shadows_only_off = GpuEffects {
        shadows: false,
        parallax: true,
    };
    assert_eq!(
        first.frame(&view(), shadows_only_off, &contents),
        second.frame(&view(), OFF, &contents),
        "shadows off with parallax on, and both off"
    );
}
