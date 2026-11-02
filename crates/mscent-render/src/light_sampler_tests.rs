use super::*;
use crate::lightmap_mips::LightMips;
use mscent_world::lightmap::{BakeConfig, LightFace, Lightmap, PointLight, bake};
use mscent_world::{Brush, Op, compile};

use mscent_math::Vec3d;

/// A lit floor and wall with odd luxel counts (so the mips have ragged
/// edges) side by side in the atlas.
fn baked() -> Lightmap {
    let room = Brush::cuboid(
        Vec3d::new(-3.0, -3.0, -3.0),
        Vec3d::new(3.0, 3.0, 3.0),
        Op::Subtract,
        0,
    )
    .unwrap();
    let bsp = compile(&[room]).unwrap();
    let quad = |pts: [[f32; 3]; 4], n: [f32; 3]| LightFace {
        vertices: pts.iter().map(|p| Vec3::new(p[0], p[1], p[2])).collect(),
        normal: Vec3::new(n[0], n[1], n[2]),
        density: Default::default(),
    };
    let floor = quad(
        [
            [-3.0, -3.0, -3.0],
            [3.0, -3.0, -3.0],
            [3.0, -3.0, 3.0],
            [-3.0, -3.0, 3.0],
        ],
        [0.0, 1.0, 0.0],
    );
    let wall = quad(
        [
            [-3.0, -3.0, -3.0],
            [3.0, -3.0, -3.0],
            [3.0, 3.0, -3.0],
            [-3.0, 3.0, -3.0],
        ],
        [0.0, 0.0, 1.0],
    );
    let lamp = PointLight {
        position: Vec3::new(1.0, -1.0, 2.0),
        color: Vec3::new(8.0, 4.0, 2.0),
        radius: 20.0,
    };
    let cfg = BakeConfig {
        luxel_size: 0.37,
        workers: 2,
        ..BakeConfig::default()
    };
    bake(&bsp, &[floor, wall], &[lamp], &cfg).unwrap()
}

/// Per-channel range of the colours of one face's own grid.
fn range(lm: &Lightmap, face: usize) -> ([f32; 3], [f32; 3]) {
    let g = lm.face_grid(face).unwrap();
    let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
    for y in 0..g.height() {
        for x in 0..g.width() {
            let c = g.at_clamped(x, y).color;
            for (k, v) in [c.x, c.y, c.z].into_iter().enumerate() {
                lo[k] = lo[k].min(v);
                hi[k] = hi[k].max(v);
            }
        }
    }
    (lo, hi)
}

#[test]
fn fetches_never_leave_the_faces_own_colour_range_at_any_level() {
    let lm = baked();
    let mips = LightMips::build(&lm);
    for face in 0..2 {
        let g = lm.face_grid(face).unwrap();
        let (lo, hi) = range(&lm, face);
        assert!(hi[0] > lo[0], "face {face} must vary for the test to bite");
        let sampler = FaceSampler::new(g, mips.levels(face));
        let (w, h) = (g.width() as f32, g.height() as f32);
        for lod in [0.0, 0.4, 1.0, 1.5, 2.7, 4.0, 9.0] {
            let plan = sampler.plan(lod);
            let mut cells = [Cell::EMPTY; 2];
            let mut out = [[0.0; 4]; 16];
            // Rows from well outside the face on every side.
            for row in -40..(h as i32 * 2 + 40) {
                let y = row as f32 * 0.5 - 3.0;
                for x0 in [-40.0, -9.0, 0.0, w - 8.0, w + 3.0] {
                    plan.colors((x0, y), (0.5, 0.0), 1.0, &mut cells, &mut out);
                    for c in out {
                        for k in 0..3 {
                            let eps = 1e-3 * hi[k].abs().max(1.0);
                            assert!(
                                c[k] >= lo[k] - eps && c[k] <= hi[k] + eps,
                                "face {face} lod {lod} at ({x0}, {y}): {c:?} outside {lo:?}..{hi:?}"
                            );
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn a_run_equals_its_pixels_fetched_one_by_one() {
    let lm = baked();
    let mips = LightMips::build(&lm);
    let g = lm.face_grid(0).unwrap();
    let sampler = FaceSampler::new(g, mips.levels(0));
    for lod in [0.0, 0.7, 1.3, 3.2] {
        let plan = sampler.plan(lod);
        let (start, step) = ((1.3, 2.9), (0.37, 0.21));
        let mut run = [[0.0; 4]; 16];
        // Cells left over from other levels must not leak into the run.
        let mut cells = [Cell::EMPTY; 2];
        plan.colors((9.0, 9.0), (1.0, 1.0), 1.0, &mut cells, &mut run);
        plan.colors(start, step, 1.0, &mut cells, &mut run);
        for (i, got) in run.iter().enumerate() {
            let mut one = [[0.0; 4]; 1];
            let at = (start.0 + step.0 * i as f32, start.1 + step.1 * i as f32);
            plan.colors(at, (0.0, 0.0), 1.0, &mut [Cell::EMPTY; 2], &mut one);
            for k in 0..3 {
                assert!(
                    (got[k] - one[0][k]).abs() <= 1e-4 * one[0][k].abs().max(1.0),
                    "lod {lod} pixel {i}: {got:?} vs {:?}",
                    one[0]
                );
            }
        }
    }
}

#[test]
fn hostile_coordinates_give_finite_colours_without_panicking() {
    let lm = baked();
    let mips = LightMips::build(&lm);
    let sampler = FaceSampler::new(lm.face_grid(1).unwrap(), mips.levels(1));
    for lod in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, -3.0, 0.5] {
        let plan = sampler.plan(lod);
        for (s, d) in [
            ((f32::NAN, 0.0), (0.0, 0.0)),
            ((f32::INFINITY, f32::NEG_INFINITY), (1.0, 1.0)),
            ((1e30, -1e30), (1e30, 1e30)),
            ((0.0, 0.0), (f32::NAN, f32::INFINITY)),
        ] {
            let mut out = [[0.0; 4]; 16];
            plan.colors(s, d, 256.0, &mut [Cell::EMPTY; 2], &mut out);
            assert!(out.iter().flatten().all(|v| v.is_finite()), "{s:?} {d:?}");
        }
    }
}
