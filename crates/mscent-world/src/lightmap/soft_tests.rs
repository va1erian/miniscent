//! Soft-shadow quality: the penumbra must be a clean monotone ramp without
//! luxel-scale steps, and a rebake must give the same bytes.

use super::*;
use crate::brush::{Brush, Op};
use crate::csg::compile;
use mscent_math::{Vec3, Vec3d};

fn v3(x: f32, y: f32, z: f32) -> Vec3 {
    Vec3::new(x, y, z)
}

/// A 20x20 floor under a plate floating at z 3, lit by a lamp at (6, 10, 9).
fn scene(workers: usize, samples: u32, radius: f32) -> Lightmap {
    let cube = |lo: [f64; 3], hi: [f64; 3], op, order| {
        Brush::cuboid(
            Vec3d::new(lo[0], lo[1], lo[2]),
            Vec3d::new(hi[0], hi[1], hi[2]),
            op,
            order,
        )
        .unwrap()
    };
    let bsp = compile(&[
        cube([0.0; 3], [20.0, 20.0, 12.0], Op::Subtract, 0),
        cube([10.0, 5.0, 3.0], [16.0, 15.0, 3.3], Op::Add, 1),
    ])
    .unwrap();
    let floor = LightFace {
        vertices: vec![
            v3(0.0, 0.0, 0.0),
            v3(20.0, 0.0, 0.0),
            v3(20.0, 20.0, 0.0),
            v3(0.0, 20.0, 0.0),
        ],
        normal: v3(0.0, 0.0, 1.0),
    };
    let lamps = [
        PointLight {
            position: v3(6.0, 10.0, 9.0),
            color: v3(1.0, 1.0, 1.0),
            radius: 40.0,
        },
        PointLight {
            position: v3(4.0, 3.0, 7.0),
            color: v3(0.0, 0.6, 0.2),
            radius: 40.0,
        },
    ];
    let cfg = BakeConfig {
        luxel_size: 0.25,
        workers,
        ambient: Vec3::ZERO,
        albedo: 0.0,
        light_radius: radius,
        shadow_samples: samples,
        ..BakeConfig::default()
    };
    bake(&bsp, &[floor], &lamps, &cfg).unwrap()
}

/// Brightness along y = 13 across the shadow edge on the +x side of the lamp
/// (the plate's near edge projects to roughly x = 12.5 on the floor).
fn row(map: &Lightmap) -> Vec<f32> {
    (0..80)
        .map(|x| {
            let p = v3((x as f32 + 0.5) * 0.25, 13.1, 0.0);
            map.sample_world(0, p).unwrap().color.x
        })
        .collect()
}

#[test]
fn penumbra_is_monotone_with_no_luxel_steps() {
    let map = scene(2, 64, 0.5);
    let r = row(&map);
    // The shadow starts where the row falls well below its lit neighbours.
    let top = r[40];
    let edge = r
        .iter()
        .position(|&v| v < top * 0.5)
        .expect("a shadow edge");
    let (lit, dark) = (r[edge - 12], r[edge + 12]);
    assert!(dark < lit * 0.2, "row {r:?}");
    let range = lit - dark;
    // Near the edge: no ripple larger than 3% of the step, steps all small.
    for w in r[edge - 8..edge + 8].windows(2) {
        assert!(
            w[1] <= w[0] + 0.03 * range,
            "ripple in {:?}",
            &r[edge - 8..edge + 8]
        );
        assert!(
            w[0] - w[1] < 0.45 * range,
            "luxel step in {:?}",
            &r[edge - 8..edge + 8]
        );
    }
    // And the ramp is spread over several luxels.
    let ramp = r
        .iter()
        .filter(|&&v| v > dark + 0.1 * range && v < lit - 0.1 * range);
    assert!(ramp.count() >= 3, "row {r:?}");
}

#[test]
fn soft_bake_is_deterministic_for_any_worker_count() {
    let a = scene(1, 64, 0.5);
    let b = scene(1, 64, 0.5);
    let c = scene(5, 64, 0.5);
    assert_eq!(a.color_ppm(), b.color_ppm());
    assert_eq!(a.texel_slice(), c.texel_slice());
}

#[test]
fn filtered_directions_stay_unit_and_finite() {
    let map = scene(2, 64, 0.5);
    for l in map.texel_slice() {
        assert!(l.color.is_finite());
        assert!((l.dir.length() - 1.0).abs() < 1e-4, "{:?}", l.dir);
    }
}

#[test]
fn smoothing_never_crosses_faces() {
    // A dark neighbour must not darken the lit face's border luxels.
    let map = scene(2, 64, 0.5);
    let p = map.projections()[0];
    let edge = map.face_luxel(0, 0, 0).unwrap().color.x;
    let inner = map.face_luxel(0, 2, 2).unwrap().color.x;
    assert!(edge > inner * 0.5, "{edge} vs {inner}");
    assert_eq!((p.width, p.height), (80, 80));
}
