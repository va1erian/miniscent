//! Baked lighting must not crawl: moving the camera by an eighth of a pixel
//! per frame changes a surface's shading smoothly. The metric and the
//! reasoning are in `support/stability.rs`; `examples/stability.rs` prints
//! the full table. Before the fix the flat-texture mean second difference was
//! 0.64 to 0.69 levels everywhere (noise that moved with the span grid) and
//! 0.48% of mid-distance samples jumped by more than 3 levels.

#[path = "support/stability.rs"]
mod stability;

use mscent_render::Quality;
use mscent_world::showcase_level;
use stability::{Look, Motion, Pose, build, measure, poses};

fn pose(name: &str) -> Pose {
    poses().into_iter().find(|p| p.name == name).expect("pose")
}

#[test]
fn lighting_only_shimmer_is_far_below_the_old_noise_floor() {
    let r = build(&showcase_level(), Look::Flat);
    for name in ["mid", "far"] {
        for motion in [Motion::Strafe, Motion::Turn] {
            let s = measure(&r, &pose(name), motion, Quality::Low);
            println!("{name} {motion:?}: {s:?}");
            assert!(s.compared > 1_000_000, "compared too few pixels");
            assert!(s.mean < 0.2, "{name} {motion:?} mean {}", s.mean);
            assert!(s.pct_over < 0.6, "{name} {motion:?} over {}", s.pct_over);
            // Real edges (polygon borders) still produce big jumps; a
            // lightmap that popped would exceed the contrast of the scene.
            assert!(s.max < 60.0 && s.ms > 0.0, "{name} {motion:?}: {s:?}");
        }
    }
}

#[test]
fn textured_bump_and_specular_do_not_add_much_shimmer() {
    let r = build(&showcase_level(), Look::Real);
    let s = measure(&r, &pose("far"), Motion::Strafe, Quality::High);
    println!("far High: {s:?}");
    // Textures have real contrast, so this bound is looser; it was 0.86 and
    // 2.0% before.
    assert!(s.mean < 0.6, "mean {}", s.mean);
    assert!(s.pct_over < 1.6, "over {}", s.pct_over);
}

#[test]
fn the_inspection_frame_is_a_png_of_twice_the_size() {
    let r = build(&showcase_level(), Look::Flat);
    let png = stability::first_frame_png(&r, &pose("mid"), Quality::Low);
    assert_eq!(&png[1..4], b"PNG");
    let be = |at: usize| u32::from_be_bytes([png[at], png[at + 1], png[at + 2], png[at + 3]]);
    assert_eq!(
        (be(16), be(20)),
        (stability::WIDTH * 2, stability::HEIGHT * 2)
    );
}
