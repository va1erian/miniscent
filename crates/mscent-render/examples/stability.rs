//! Prints the temporal shimmer of the baked lighting for the showcase level.
//!
//! `cargo run --release -p mscent-render --example stability`
//!
//! For each camera distance and motion (strafe, turn, 1/8 pixel per frame)
//! it reports the mean and the share of large second differences of pixel
//! luminance; see `tests/support/stability.rs` for the exact definition.

#[path = "../tests/support/stability.rs"]
mod stability;

use mscent_render::Quality;
use mscent_world::showcase_level;
use stability::{Look, Motion, build, measure, poses};

fn main() {
    let level = showcase_level();
    let out = std::path::PathBuf::from(
        std::env::args_os()
            .nth(1)
            .unwrap_or_else(|| "target/shots/stability".into()),
    );
    std::fs::create_dir_all(&out).expect("create output dir");
    let real = build(&level, Look::Real);
    for pose in poses() {
        let png = stability::first_frame_png(&real, &pose, Quality::High);
        std::fs::write(out.join(format!("real_{}.png", pose.name)), png).expect("write png");
    }
    for look in [Look::Flat, Look::Real] {
        let r = build(&level, look);
        for q in [Quality::Low, Quality::High] {
            for pose in poses() {
                for motion in [Motion::Strafe, Motion::Turn] {
                    let s = measure(&r, &pose, motion, q);
                    println!(
                        "{look:?} {q:?} {:>4} {motion:?}: mean {:.3} over{} {:.2}% max {:.1} n={} {:.1} ms",
                        pose.name,
                        s.mean,
                        stability::SHIMMER,
                        s.pct_over,
                        s.max,
                        s.compared,
                        s.ms
                    );
                }
            }
        }
    }
}
