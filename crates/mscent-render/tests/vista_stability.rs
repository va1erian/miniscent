//! A vista must not shimmer more than a wall when the camera moves: its towers are
//! thin at this distance. Uses the same metric as `tests/stability.rs`, on the
//! Monument demo with the real textures.

#[path = "support/stability.rs"]
mod stability;

use std::path::Path;

use mscent_math::Vec3;
use mscent_render::Quality;
use mscent_world::format;
use stability::{Look, Motion, Pose, build, measure};

#[test]
fn the_monument_is_steady_at_an_eighth_of_a_pixel_a_frame() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/levels/facing.msl");
    let level = format::read(&std::fs::read(path).expect("the demo")).expect("a level");
    let r = build(&level, Look::Real);
    let pose = Pose {
        name: "gate",
        eye: Vec3::new(0.0, -22.0, 0.0),
        yaw: 0.0,
        pitch: 0.0,
        dist: 60.0,
    };
    for motion in [Motion::Strafe, Motion::Turn] {
        let s = measure(&r, &pose, motion, Quality::Low);
        println!("{motion:?}: {s:?}");
        assert!(s.compared > 1_000_000, "compared too few pixels");
        assert!(s.mean < 0.6, "{motion:?} mean {}", s.mean);
        assert!(s.pct_over < 1.6, "{motion:?} over {}", s.pct_over);
    }
}
