//! Operators that read their input somewhere else than where they write it.
//! Every read goes through [`crate::image::Frame::sample`], so the window can
//! move. Each keeps the period of its input: whole zooms, or a warp that is zero
//! before the tile's edge.

use super::cost::Cost;
use super::{Ctx, OpSpec, Role, exactly, float, int};
use crate::error::TexError;
use crate::image::{Image, to_px};
use crate::math::{sin_cos, wrap_delta};

/// The centre of twirl and bulge. Pinned, because a warp about another point
/// would reach the tile's edge for the same radius.
const CENTRE: f32 = 0.5;

/// Quarter turns clockwise (y runs down), about the tile's centre, then a whole
/// zoom, then a shift. The zoom makes the source repeat `zoom` times across the
/// tile; a shift of any size keeps the period, so it is free.
fn rotozoom(ctx: &Ctx) -> Result<Image, TexError> {
    let source = ctx.input(0)?;
    let quarters = ctx.int("turns").rem_euclid(4);
    let (zx, zy) = (ctx.int("zoom_x") as f32, ctx.int("zoom_y") as f32);
    let (sx, sy) = (ctx.float("shift_x"), ctx.float("shift_y"));
    Ok(Image::from_xy(ctx.frame.size, |x, y| {
        let (u, v) = ctx.frame.uv(x, y);
        let (qx, qy) = (u - CENTRE, v - CENTRE);
        let (rx, ry) = match quarters {
            0 => (qx, qy),
            1 => (qy, -qx),
            2 => (-qx, -qy),
            _ => (-qy, qx),
        };
        to_px(
            ctx.frame
                .sample(source, rx * zx + CENTRE + sx, ry * zy + CENTRE + sy),
        )
    }))
}

/// A warp about the tile's centre: `offset` maps a texel's wrapped offset from
/// the centre and its distance, to where to read from. Beyond `radius` the read
/// is the texel itself, which is exactly the input; `radius` is at most half a
/// tile, so the warp never reaches the edge.
fn about_centre(
    ctx: &Ctx,
    radius: f32,
    offset: impl Fn(f32, f32, f32) -> (f32, f32),
) -> Result<Image, TexError> {
    let source = ctx.input(0)?;
    Ok(Image::from_xy(ctx.frame.size, |x, y| {
        let (u, v) = ctx.frame.uv(x, y);
        let (dx, dy) = (wrap_delta(u - CENTRE), wrap_delta(v - CENTRE));
        let d = (dx * dx + dy * dy).sqrt();
        if d >= radius {
            return source.at(x, y);
        }
        let (nx, ny) = offset(dx, dy, 1.0 - d / radius);
        to_px(ctx.frame.sample(source, u - dx + nx, v - dy + ny))
    }))
}

/// Turns the picture about the centre, most at the middle and by nothing at the
/// radius.
fn twirl(ctx: &Ctx) -> Result<Image, TexError> {
    let strength = ctx.float("strength");
    about_centre(ctx, ctx.float("radius"), |dx, dy, falloff| {
        let (s, c) = sin_cos(strength * falloff * falloff);
        (dx * c - dy * s, dx * s + dy * c)
    })
}

/// Magnifies (positive) or pinches (negative) around the centre.
fn bulge(ctx: &Ctx) -> Result<Image, TexError> {
    let strength = ctx.float("strength");
    about_centre(ctx, ctx.float("radius"), |dx, dy, falloff| {
        let scale = 1.0 - strength * falloff * falloff;
        (dx * scale, dy * scale)
    })
}

/// Shifts each texel's read by the map's red and green around one half.
fn distort(ctx: &Ctx) -> Result<Image, TexError> {
    let (source, map) = (ctx.input(0)?, ctx.input(1)?);
    let amount = ctx.float("amount");
    Ok(Image::from_xy(ctx.frame.size, |x, y| {
        let (u, v) = ctx.frame.uv(x, y);
        let m = map.at(x, y);
        let shift = |c: u16| (f32::from(c) / 65535.0 - 0.5) * amount;
        to_px(ctx.frame.sample(source, u + shift(m[0]), v + shift(m[1])))
    }))
}

pub(super) const OPS: &[OpSpec] = &[
    OpSpec {
        name: "rotozoom",
        group: "Filter",
        inputs: exactly(1),
        params: &[
            int("turns", -8, 8, 0),
            int("zoom_x", 1, 16, 1),
            int("zoom_y", 1, 16, 1),
            float("shift_x", 0.0, 1.0, 0.0),
            float("shift_y", 0.0, 1.0, 0.0),
        ],
        role: Role::Kernel,
        kernel: rotozoom,
        cost: Cost::Flat(60.0),
    },
    OpSpec {
        name: "twirl",
        group: "Filter",
        inputs: exactly(1),
        params: &[
            float("strength", -8.0, 8.0, 2.0),
            float("radius", 0.0, 0.5, 0.4),
        ],
        role: Role::Kernel,
        kernel: twirl,
        cost: Cost::Flat(58.0),
    },
    OpSpec {
        name: "bulge",
        group: "Filter",
        inputs: exactly(1),
        params: &[
            float("strength", -1.0, 1.0, 0.5),
            float("radius", 0.0, 0.5, 0.4),
        ],
        role: Role::Kernel,
        kernel: bulge,
        cost: Cost::Flat(40.0),
    },
    OpSpec {
        name: "distort",
        group: "Filter",
        inputs: exactly(2),
        params: &[float("amount", 0.0, 0.5, 0.1)],
        role: Role::Kernel,
        kernel: distort,
        cost: Cost::Flat(54.0),
    },
];
