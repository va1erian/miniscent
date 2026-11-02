//! Shapes drawn over the input (or a background colour). Positions are
//! normalized and distances wrapped, so a shape near an edge shows on both sides.

use mscent_math::epsilon::TEXTURE_MIN_ELLIPSE_AXIS;

use super::cost::Cost;
use super::util::{BLACK, WHITE, coverage, draw, texel, unit_hash};
use super::{Ctx, Inputs, OpSpec, Role, color, float, int};
use crate::error::TexError;
use crate::image::Image;
use crate::math::wrap_delta;

/// A drawing operator takes at most one input to draw over.
const OPTIONAL: Inputs = Inputs { min: 0, max: 1 };

/// Signed distance to a box of half-sides `hx`, `hy` centred on the origin.
fn box_distance(dx: f32, dy: f32, hx: f32, hy: f32) -> f32 {
    let (qx, qy) = (dx.abs() - hx, dy.abs() - hy);
    let outside = (qx.max(0.0) * qx.max(0.0) + qy.max(0.0) * qy.max(0.0)).sqrt();
    outside + qx.max(qy).min(0.0)
}

fn rect(ctx: &Ctx) -> Result<Image, TexError> {
    let (x, y) = (ctx.float("x"), ctx.float("y"));
    let (hw, hh) = (ctx.float("w") / 2.0, ctx.float("h") / 2.0);
    let (soft, px) = (ctx.float("softness"), texel(ctx));
    Ok(draw(ctx, |u, v| {
        let sd = box_distance(wrap_delta(u - x), wrap_delta(v - y), hw, hh);
        coverage(sd, soft, px)
    }))
}

/// Not the exact distance to an ellipse, which has no closed form, but the
/// implicit function scaled to tile units: right on the axes and close elsewhere.
fn ellipse(ctx: &Ctx) -> Result<Image, TexError> {
    let (x, y) = (ctx.float("x"), ctx.float("y"));
    let (rx, ry) = (
        (ctx.float("w") / 2.0).max(TEXTURE_MIN_ELLIPSE_AXIS),
        (ctx.float("h") / 2.0).max(TEXTURE_MIN_ELLIPSE_AXIS),
    );
    let (soft, px) = (ctx.float("softness"), texel(ctx));
    Ok(draw(ctx, |u, v| {
        let (dx, dy) = (wrap_delta(u - x) / rx, wrap_delta(v - y) / ry);
        let k = (dx * dx + dy * dy).sqrt();
        coverage((k - 1.0) * rx.min(ry), soft, px)
    }))
}

/// Distance from `p` to the segment from the origin to `d`.
fn segment_distance(p: (f32, f32), d: (f32, f32)) -> f32 {
    let len2 = d.0 * d.0 + d.1 * d.1;
    let t = if len2 > 0.0 {
        ((p.0 * d.0 + p.1 * d.1) / len2).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let (ex, ey) = (p.0 - d.0 * t, p.1 - d.1 * t);
    (ex * ex + ey * ey).sqrt()
}

/// A line segment between two points of the tile, thick and wrapped: its copies
/// in the eight neighbouring tiles reach in over the edges.
fn line(ctx: &Ctx) -> Result<Image, TexError> {
    let (x0, y0) = (ctx.float("x0"), ctx.float("y0"));
    let d = (ctx.float("x1") - x0, ctx.float("y1") - y0);
    let (half, soft, px) = (ctx.float("width") / 2.0, ctx.float("softness"), texel(ctx));
    Ok(draw(ctx, |u, v| {
        let (wx, wy) = (wrap_delta(u - x0), wrap_delta(v - y0));
        let mut nearest = f32::MAX;
        for j in -1..=1 {
            for i in -1..=1 {
                let p = (wx + i as f32, wy + j as f32);
                nearest = nearest.min(segment_distance(p, d));
            }
        }
        coverage(nearest - half, soft, px)
    }))
}

/// A grid of round stamps, each moved within its cell by a seeded jitter. A texel
/// looks at its own cell and the eight around it, which is all a stamp that
/// stays near its cell can reach.
fn dots(ctx: &Ctx) -> Result<Image, TexError> {
    let (nx, ny) = (ctx.int("count_x"), ctx.int("count_y"));
    let (radius, soft) = (ctx.float("radius"), ctx.float("softness"));
    let slack = ctx.float("jitter") * (0.5 - radius).max(0.0);
    let seed = ctx.int("seed") as u32;
    let size = ctx.frame.size;
    // Distances below are in cells, so the edge needs its texel in cells too.
    let px = (nx as f32 / size.width() as f32).max(ny as f32 / size.height() as f32);
    Ok(draw(ctx, |u, v| {
        let (cu, cv) = (u * nx as f32, v * ny as f32);
        let (ci, cj) = (cu.floor() as i32, cv.floor() as i32);
        let mut best: f32 = 0.0;
        for gj in cj - 1..=cj + 1 {
            for gi in ci - 1..=ci + 1 {
                let (wi, wj) = (gi.rem_euclid(nx), gj.rem_euclid(ny));
                let jx = (unit_hash(wi, wj, seed) - 0.5) * 2.0 * slack;
                let jy = (unit_hash(wi, wj, seed ^ 0x5bd1_e995) - 0.5) * 2.0 * slack;
                let (dx, dy) = (cu - (gi as f32 + 0.5 + jx), cv - (gj as f32 + 0.5 + jy));
                let d = (dx * dx + dy * dy).sqrt();
                best = best.max(coverage(d - radius, soft, px));
            }
        }
        best
    }))
}

/// The parameters every shape shares, after its own.
macro_rules! shape_params {
    ($($own:expr),* $(,)?) => {
        &[$($own,)* color("color", WHITE), color("background", BLACK)]
    };
}

pub(super) const OPS: &[OpSpec] = &[
    OpSpec {
        name: "rect",
        group: "Draw",
        inputs: OPTIONAL,
        params: shape_params![
            float("x", 0.0, 1.0, 0.5),
            float("y", 0.0, 1.0, 0.5),
            float("w", 0.0, 1.0, 0.5),
            float("h", 0.0, 1.0, 0.25),
            float("softness", 0.0, 0.5, 0.0),
        ],
        role: Role::Kernel,
        kernel: rect,
        cost: Cost::Flat(16.0),
    },
    OpSpec {
        name: "ellipse",
        group: "Draw",
        inputs: OPTIONAL,
        params: shape_params![
            float("x", 0.0, 1.0, 0.5),
            float("y", 0.0, 1.0, 0.5),
            float("w", 0.0, 1.0, 0.5),
            float("h", 0.0, 1.0, 0.25),
            float("softness", 0.0, 0.5, 0.0),
        ],
        role: Role::Kernel,
        kernel: ellipse,
        cost: Cost::Flat(18.0),
    },
    OpSpec {
        name: "line",
        group: "Draw",
        inputs: OPTIONAL,
        params: shape_params![
            float("x0", 0.0, 1.0, 0.1),
            float("y0", 0.0, 1.0, 0.2),
            float("x1", 0.0, 1.0, 0.9),
            float("y1", 0.0, 1.0, 0.7),
            float("width", 0.0, 0.5, 0.04),
            float("softness", 0.0, 0.5, 0.0),
        ],
        role: Role::Kernel,
        kernel: line,
        cost: Cost::Flat(42.0),
    },
    OpSpec {
        name: "dots",
        group: "Draw",
        inputs: OPTIONAL,
        params: shape_params![
            int("count_x", 1, 64, 8),
            int("count_y", 1, 64, 8),
            float("radius", 0.0, 0.5, 0.25),
            float("softness", 0.0, 0.5, 0.0),
            float("jitter", 0.0, 1.0, 0.0),
            int("seed", 0, 65535, 1),
        ],
        role: Role::Kernel,
        kernel: dots,
        cost: Cost::Flat(150.0),
    },
];
