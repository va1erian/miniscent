//! Cellular patterns and small band noise. Both are periodic by construction:
//! points are looked up by wrapped cell index, waves have whole cycles per tile.

use mscent_math::rng::Rng;

use super::cost::{Cost, Knobs};
use super::util::{BLACK, WHITE, mix, unit_hash};
use super::{Ctx, OpSpec, Role, choice, color, exactly, int};
use crate::error::TexError;
use crate::image::Image;
use crate::math::frac;

/// Cells per side for `count` points. The points sit one per cell of a square
/// grid, so the real count is the nearest square; that bounds what a texel has
/// to look at to its eight neighbours.
fn grid_side(count: i32) -> i32 {
    ((count as f32).sqrt() + 0.5).floor().max(1.0) as i32
}

/// One jittered point per grid cell, in cell units. Kept off the cell's edge so
/// that the nearest point is almost always in the 3x3 neighbourhood.
fn scatter(side: i32, seed: u32) -> Vec<[f32; 2]> {
    let mut rng = Rng::new(u64::from(seed) + 1);
    (0..side * side)
        .map(|_| [rng.range_f32(0.1, 0.9), rng.range_f32(0.1, 0.9)])
        .collect()
}

/// The two nearest points of a texel, in cell units, and the nearer's cell.
struct Nearest {
    first: f32,
    second: f32,
    cell: (i32, i32),
}

fn nearest(points: &[[f32; 2]], side: i32, u: f32, v: f32) -> Nearest {
    let (px, py) = (u * side as f32, v * side as f32);
    let (ci, cj) = (px.floor() as i32, py.floor() as i32);
    let mut best = Nearest {
        first: f32::MAX,
        second: f32::MAX,
        cell: (0, 0),
    };
    for dj in -1..=1 {
        for di in -1..=1 {
            // Neighbours are named by their unwrapped index, so a one-cell grid
            // still sees its own copies on every side.
            let (gi, gj) = (ci + di, cj + dj);
            let p = points[((gj.rem_euclid(side)) * side + gi.rem_euclid(side)) as usize];
            let (dx, dy) = (gi as f32 + p[0] - px, gj as f32 + p[1] - py);
            let d = dx * dx + dy * dy;
            if d < best.first {
                best.second = best.first;
                best.first = d;
                best.cell = (gi.rem_euclid(side), gj.rem_euclid(side));
            } else if d < best.second {
                best.second = d;
            }
        }
    }
    best.first = best.first.sqrt();
    best.second = best.second.sqrt();
    best
}

fn cells(ctx: &Ctx) -> Result<Image, TexError> {
    let side = grid_side(ctx.int("count"));
    let seed = ctx.int("seed") as u32;
    let points = scatter(side, seed);
    let (a, b) = (ctx.color("color0"), ctx.color("color1"));
    let mode = ctx.choice("mode");
    Ok(ctx.frame.fill(|u, v| {
        let n = nearest(&points, side, u, v);
        let t = match mode {
            // Distances are in cells, so a recipe keeps its look as the count
            // changes. Borders are where the two nearest points are equally far.
            0 => n.first,
            1 => (n.second - n.first) * 8.0,
            _ => unit_hash(n.cell.0, n.cell.1, seed),
        };
        mix(a, b, t.clamp(0.0, 1.0))
    }))
}

/// A triangle wave `-1.0..=1.0` with period 1.
fn triangle(x: f32) -> f32 {
    4.0 * (frac(x) - 0.5).abs() - 1.0
}

/// Directions of the wave lattices, as whole numbers of cycles.
const DIRECTIONS: [(f32, f32); 4] = [(1.0, 0.0), (0.0, 1.0), (1.0, 1.0), (1.0, -1.0)];

/// Octaves of four triangle waves with seeded phases. Not a true wavelet noise,
/// only a cheap band-limited-looking texture that tiles exactly.
fn wavelet(ctx: &Ctx) -> Result<Image, TexError> {
    let (freq, octaves) = (ctx.int("freq"), ctx.int("octaves"));
    let seed = ctx.int("seed") as u32;
    let (a, b) = (ctx.color("color0"), ctx.color("color1"));
    // Each octave has half the weight of the one before; the total keeps the sum
    // inside the colour range.
    let (mut total, mut weight) = (0.0, 1.0);
    for _ in 0..octaves {
        total += weight * DIRECTIONS.len() as f32;
        weight *= 0.5;
    }
    Ok(ctx.frame.fill(|u, v| {
        let (mut sum, mut weight) = (0.0, 1.0);
        for o in 0..octaves {
            let scale = (freq << o) as f32;
            for (k, (dx, dy)) in DIRECTIONS.iter().enumerate() {
                let phase = unit_hash(o, k as i32, seed);
                sum += triangle(scale * (dx * u + dy * v) + phase) * weight;
            }
            weight *= 0.5;
        }
        mix(a, b, sum / total * 0.5 + 0.5)
    }))
}

/// Nanoseconds per texel of `wavelet`: its fill, and each octave's four waves.
fn wavelet_cost(k: &Knobs) -> f64 {
    19.0 + 6.0 * f64::from(k.int("octaves"))
}

pub(super) const OPS: &[OpSpec] = &[
    OpSpec {
        name: "cells",
        group: "Pattern",
        inputs: exactly(0),
        params: &[
            int("count", 1, 256, 16),
            int("seed", 0, 65535, 1),
            choice("mode", &["distance", "edge", "id"], 0),
            color("color0", BLACK),
            color("color1", WHITE),
        ],
        role: Role::Kernel,
        kernel: cells,
        cost: Cost::Flat(32.0),
    },
    OpSpec {
        name: "wavelet",
        group: "Pattern",
        inputs: exactly(0),
        params: &[
            int("freq", 1, 64, 4),
            int("octaves", 1, 6, 3),
            int("seed", 0, 65535, 1),
            color("color0", BLACK),
            color("color1", WHITE),
        ],
        role: Role::Kernel,
        kernel: wavelet,
        cost: Cost::Worked(wavelet_cost),
    },
];
