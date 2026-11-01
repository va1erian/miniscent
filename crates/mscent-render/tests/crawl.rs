//! Shadow edges must neither crawl nor jag. The metric is in
//! `support/crawl.rs`; `examples/crawl.rs` prints the full table and PNGs.
//!
//! Measured on the code before the shadow fix (flat albedo, Low quality,
//! 640x360, 1/4 pixel per frame, mean of dolly, strafe and yaw): the contour
//! of a floor shadow edge deviates from a straight line by 0.68 (close),
//! 0.52 (near), 0.26 (mid) and 0.12 (far) pixels RMS, up to 3.1 pixels peak
//! to peak, while the luminance of followed world points is steady to 0.2 to
//! 0.5 levels. The edge is a world-fixed staircase of luxel size (the bake
//! stores a one-luxel step and bilinear sampling turns it into zigzags), so
//! it slides along the edge when the camera moves.

#[allow(dead_code)]
#[path = "support/crawl.rs"]
mod crawl;
#[allow(dead_code)]
#[path = "support/stability.rs"]
mod stability;

use crawl::{COARSE, FULL, Motion, REDUCED, Rig};
use mscent_render::Quality;
use mscent_world::showcase_level;
use stability::Look;

fn rig(name: &str) -> Rig {
    crawl::rigs()
        .into_iter()
        .find(|r| r.name == name)
        .expect("rig")
}

#[test]
fn the_line_fit_sees_a_zigzag_and_ignores_a_tilt() {
    let line: Vec<(f32, f32)> = (0..20).map(|i| (i as f32, 0.5 * i as f32 + 3.0)).collect();
    let (rms, pp) = crawl::line_rms(&line);
    assert!(rms < 1e-3 && pp < 1e-3, "{rms} {pp}");
    let zigzag: Vec<(f32, f32)> = (0..20)
        .map(|i| (i as f32, 3.0 + if i % 2 == 0 { 1.0 } else { -1.0 }))
        .collect();
    let (rms, pp) = crawl::line_rms(&zigzag);
    assert!(
        (rms - 1.0).abs() < 0.1 && (1.9..2.4).contains(&pp),
        "{rms} {pp}"
    );
}

#[test]
fn every_rig_finds_its_shadow_edge_and_follows_all_its_points() {
    let r = stability::build(&showcase_level(), Look::Flat);
    for name in ["room_near", "cave_mid"] {
        let run = crawl::run(
            &r,
            &rig(name),
            Motion::Strafe,
            Quality::Low,
            FULL,
            COARSE,
            true,
        )
        .expect("run");
        let c = run.crawl;
        println!("{name}: {c:?}");
        assert_eq!(c.points, 35, "{name}");
        assert!(
            c.contour_samples >= 38,
            "{name} found {}",
            c.contour_samples
        );
        assert_eq!(run.frames.len(), 3);
        // Sanity bounds on the current state, far above the noise floor of
        // the metric and far below a lost edge.
        assert!(c.saw_px > 0.02 && c.saw_px < 2.0, "{name} saw {}", c.saw_px);
        assert!(c.lum_rms < 1.0, "{name} lum {}", c.lum_rms);
    }
}

/// The goal: area-sampled, filtered shadow edges stay straight and steady.
#[test]
fn shadow_edges_are_straight_and_steady_at_every_distance_and_scale() {
    let r = stability::build(&showcase_level(), Look::Flat);
    // RMS deviation from a straight edge, in pixels, per rig. Close rigs
    // allow a little more since deep shadows make the edge easier to over-read.
    let goal = [
        ("room_close", 0.3),
        ("room_near", 0.3),
        ("room_mid", 0.08),
        ("room_far", 0.05),
        ("cave_mid", 0.15),
    ];
    let mut failures = Vec::new();
    for (name, max_saw) in goal {
        for size in [FULL, REDUCED] {
            let c = crawl::measure(&r, &rig(name), Quality::Low, size, COARSE).expect("measure");
            println!("{name} {size:?}: {c:?}");
            if c.saw_px > max_saw {
                failures.push(format!("{name} {size:?}: saw {:.3} > {max_saw}", c.saw_px));
            }
            if c.lum_rms_edge > 0.9 || c.contour_px > 0.06 {
                failures.push(format!("{name} {size:?}: crawl {c:?}"));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
