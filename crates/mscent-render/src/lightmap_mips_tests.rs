use super::*;
use mscent_world::lightmap::{BakeConfig, LightFace, PointLight, bake};
use mscent_world::{Brush, Op, compile};

use mscent_math::{Vec3, Vec3d};

/// A lit floor and a lit wall, so the atlas holds two faces side by side.
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
        position: Vec3::new(0.0, 0.0, 0.0),
        color: Vec3::new(8.0, 8.0, 8.0),
        radius: 20.0,
    };
    let cfg = BakeConfig {
        luxel_size: 0.25,
        workers: 2,
        ..BakeConfig::default()
    };
    bake(&bsp, &[floor, wall], &[lamp], &cfg).unwrap()
}

fn mean(grid: &ColorGrid) -> [f32; 3] {
    let (w, h) = (grid.width(), grid.height());
    let mut sum = [0.0; 3];
    for y in 0..h {
        for x in 0..w {
            let c = grid.cell(x, y)[0];
            for k in 0..3 {
                sum[k] += c[k];
            }
        }
    }
    sum.map(|s| s / (w * h) as f32)
}

#[test]
fn level_zero_is_the_baked_colour_and_the_pad_repeats_the_edge() {
    let lm = baked();
    let mips = LightMips::build(&lm);
    for face in 0..2 {
        let base = &mips.levels(face)[0];
        let grid = lm.face_grid(face).unwrap();
        assert_eq!((base.width(), base.height()), (grid.width(), grid.height()));
        for (x, y) in [(0, 0), (base.width() - 1, 3), (5, base.height() - 1)] {
            let c = grid.at_clamped(x, y).color;
            assert_eq!(&base.cell(x, y)[0][..3], &[c.x, c.y, c.z]);
        }
        let (w, h) = (base.width(), base.height());
        // The cell at the last luxel reads the repeated column and row.
        let last = base.cell(w - 1, h - 1);
        assert!(
            last.iter().all(|c| *c == last[0]),
            "pad differs from the edge"
        );
    }
}

#[test]
fn each_level_halves_and_keeps_the_mean_light() {
    let lm = baked();
    let mips = LightMips::build(&lm);
    for face in 0..2 {
        let levels = mips.levels(face);
        assert!(levels.len() > 3, "face {face} has {} levels", levels.len());
        let base = mean(&levels[0]);
        assert!(base[0] > 0.0, "the face is lit");
        for pair in levels.windows(2) {
            assert_eq!(pair[1].width(), pair[0].width().div_ceil(2));
            assert_eq!(pair[1].height(), pair[0].height().div_ceil(2));
        }
        // A box filter keeps the mean up to the edge luxels that odd sizes
        // weigh differently.
        for level in &levels[1..] {
            let m = mean(level);
            assert!((m[0] - base[0]).abs() < 0.15 * base[0], "{m:?} vs {base:?}");
        }
    }
}

#[test]
fn faces_never_mix_and_unknown_faces_have_no_levels() {
    let lm = baked();
    let mips = LightMips::build(&lm);
    let colors: Vec<Rgbx> = (0..2)
        .map(|f| mips.levels(f).last().unwrap().cell(0, 0)[0])
        .collect();
    assert_ne!(colors[0], colors[1], "the floor and the wall average alike");
    assert!(mips.levels(99).is_empty());
    assert_eq!(
        mips.levels(0).last().map(|g| (g.width(), g.height())),
        Some((1, 1))
    );
}
