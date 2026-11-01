//! Regression tests for bake artefacts found in review.

use super::*;
use crate::brush::{Brush, Op};
use crate::csg::compile;
use mscent_math::{Vec3, Vec3d};

fn room(w: f64) -> Brush {
    Brush::cuboid(
        Vec3d::new(0.0, 0.0, 0.0),
        Vec3d::new(w, 10.0, 6.0),
        Op::Subtract,
        0,
    )
    .unwrap()
}

fn cfg() -> BakeConfig {
    BakeConfig {
        luxel_size: 0.25,
        workers: 2,
        ambient: Vec3::ZERO,
        albedo: 0.0,
        ..BakeConfig::default()
    }
}

/// Floor of a `w` by 10 room lit from the middle of the ceiling.
fn lit_floor(w: f32) -> (Lightmap, usize) {
    let bsp = compile(&[room(f64::from(w))]).unwrap();
    let floor = LightFace {
        vertices: vec![
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(w, 0.0, 0.0),
            Vec3::new(w, 10.0, 0.0),
            Vec3::new(0.0, 10.0, 0.0),
        ],
        normal: Vec3::new(0.0, 0.0, 1.0),
    };
    let lamp = PointLight {
        position: Vec3::new(w / 2.0, 5.0, 5.0),
        color: Vec3::new(8.0, 8.0, 8.0),
        radius: 60.0,
    };
    (bake(&bsp, &[floor], &[lamp], &cfg()).unwrap(), 0)
}

fn brightness(lm: &Lightmap, face: usize, x: u32, y: u32) -> f32 {
    lm.face_luxel(face, x, y).unwrap().color.x
}

#[test]
fn edge_luxels_of_a_face_that_is_not_a_whole_number_of_luxels_are_lit() {
    // 10.1 / 0.25 = 40.4 luxels: the 41st centre lies 0.025 past the wall.
    let (lm, f) = lit_floor(10.1);
    let p = lm.projections()[f];
    // The face's long axis may be tangent or bitangent; look at both ends of
    // every row/column instead of assuming.
    let (w, h) = (p.width, p.height);
    let mid = |n: u32| n / 2;
    let edge_cells = [(0, mid(h)), (w - 1, mid(h)), (mid(w), 0), (mid(w), h - 1)];
    for (x, y) in edge_cells {
        let (inner_x, inner_y) = (x.clamp(1, w - 2), y.clamp(1, h - 2));
        let edge = brightness(&lm, f, x, y);
        let inner = brightness(&lm, f, inner_x, inner_y);
        assert!(
            edge > inner * 0.5,
            "edge luxel ({x},{y}) = {edge} vs next one in {inner}"
        );
    }
}
