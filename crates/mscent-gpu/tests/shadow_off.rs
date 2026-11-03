//! A frame with the shadows switched off is the frame as it was drawn before they
//! existed, byte for byte: the hashes below were taken from the renderer at the
//! commit before the shadow work (b3c0e6d), and nothing since may move them.
//!
//! A machine with no GPU skips this, unless `MSCENT_REQUIRE_GPU` is set.

#[path = "support/gpu.rs"]
mod gpu_support;
#[allow(dead_code)]
#[path = "support/shade.rs"]
mod shade;

use mscent_math::Vec3;
use shade::{Contents, OFF, Room, Shot, box_at, fnv, lamp, room, view};

fn v(x: f32, y: f32, z: f32) -> Vec3 {
    Vec3::new(x, y, z)
}

/// A lit room with a pillar and a bake, a box in the light's way and a second light.
#[test]
fn the_frame_with_shadows_off_is_the_frame_before() {
    let Some(gpu) = gpu_support::gpu() else {
        return;
    };
    let spec = Room {
        baked_lamp: Some((v(6.0, 6.0, 5.0), 2.0)),
        pillar: true,
        ..Room::default()
    };
    let mut shot = Shot::new(&gpu, room(spec));
    let lights = [
        lamp(&shot.level, v(0.0, -4.0, 3.0)),
        lamp(&shot.level, v(5.0, 2.0, 1.0)),
    ];
    let boxes = [box_at(v(-3.0, -5.0, -1.0)), box_at(v(3.0, -6.0, -2.0))];
    let contents = Contents {
        lights: &lights,
        boxes: &boxes,
        ..Contents::default()
    };
    let frame = shot.frame(&view(), OFF, &contents);
    shade::keep("off.png", &frame);
    assert_eq!(fnv(&frame), 0x4d4c_1f56_c230_8935, "the lit room");
    let plain = shot.frame(&view(), OFF, &Contents::default());
    assert_eq!(
        fnv(&plain),
        0x5963_4707_7d03_ed82,
        "the room with nothing in it"
    );
}
