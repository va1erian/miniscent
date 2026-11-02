//! Generators: operators that make a tile from nothing but their parameters.

use super::util::{BLACK, WHITE, mix};
use super::{Ctx, OpSpec, Role, choice, color, exactly, float, int};
use crate::error::TexError;
use crate::image::{Image, to_px};
use crate::math::{frac, pow01, wrap_delta};
use crate::noise::{Fold, Noise};

fn flat(ctx: &Ctx) -> Result<Image, TexError> {
    Ok(Image::flat(ctx.frame.size, to_px(ctx.color("color"))))
}

/// Raw noise over the tile, scaled to the full range so that a recipe's contrast
/// does not depend on its octaves.
fn noise(ctx: &Ctx) -> Result<Image, TexError> {
    let pattern = Noise {
        freq: (ctx.int("freq_x") as u32, ctx.int("freq_y") as u32),
        octaves: ctx.int("octaves") as u32,
        persistence: ctx.float("persistence"),
        seed: ctx.int("seed") as u32,
        fold: match ctx.choice("mode") {
            1 => Fold::Ridged,
            _ => Fold::Fbm,
        },
    };
    let size = ctx.frame.size;
    let mut raw = Vec::with_capacity(size.texels());
    let (mut low, mut high) = (f32::INFINITY, f32::NEG_INFINITY);
    for y in 0..size.height() as i32 {
        for x in 0..size.width() as i32 {
            let (u, v) = ctx.frame.uv(x, y);
            let n = pattern.at(u, v);
            low = low.min(n);
            high = high.max(n);
            raw.push(n);
        }
    }
    let span = high - low;
    let scale = if span > 0.0 { 1.0 / span } else { 0.0 };
    let (a, b) = (ctx.color("color0"), ctx.color("color1"));
    let mut values = raw.iter();
    Ok(Image::from_xy(size, |_, _| {
        let n = values.next().copied().unwrap_or(low);
        to_px(mix(a, b, (n - low) * scale))
    }))
}

/// Alternating cells. Odd counts would put two like cells side by side at the
/// seam, so they are refused.
fn checker(ctx: &Ctx) -> Result<Image, TexError> {
    let (cols, rows) = (ctx.int("cols"), ctx.int("rows"));
    if cols % 2 != 0 || rows % 2 != 0 {
        return Err(TexError::NotTiling("checker counts must be even"));
    }
    let (a, b) = (ctx.color("color0"), ctx.color("color1"));
    Ok(ctx.frame.fill(|u, v| {
        let cell = (u * cols as f32).floor() as i64 + (v * rows as f32).floor() as i64;
        if cell & 1 == 0 { a } else { b }
    }))
}

/// Length of the part of the bands `[k, k + duty)` below `x`.
fn lit_below(x: f32, duty: f32) -> f32 {
    x.floor() * duty + frac(x).min(duty)
}

/// Diagonal bands by integer cycles per tile. Each texel gets the exact share of
/// its footprint (a box, seen along the stripe direction) that falls in a band,
/// so shallow diagonals do not stair-step.
fn stripes(ctx: &Ctx) -> Result<Image, TexError> {
    let (cx, cy) = (ctx.int("cycles_x") as f32, ctx.int("cycles_y") as f32);
    let duty = ctx.float("duty");
    let (a, b) = (ctx.color("color0"), ctx.color("color1"));
    let size = ctx.frame.size;
    let footprint = cx.abs() / size.width() as f32 + cy.abs() / size.height() as f32;
    Ok(ctx.frame.fill(|u, v| {
        if footprint == 0.0 {
            return a;
        }
        let p = cx * u + cy * v;
        let half = footprint / 2.0;
        let share = (lit_below(p + half, duty) - lit_below(p - half, duty)) / footprint;
        mix(a, b, share)
    }))
}

/// A ramp along a direction that goes up and back down, so the tile's ends meet.
fn gradient(ctx: &Ctx) -> Result<Image, TexError> {
    let (cx, cy) = (ctx.int("cycles_x") as f32, ctx.int("cycles_y") as f32);
    let (a, b) = (ctx.color("color0"), ctx.color("color1"));
    Ok(ctx.frame.fill(|u, v| {
        let wave = 1.0 - (2.0 * frac(cx * u + cy * v) - 1.0).abs();
        mix(a, b, wave)
    }))
}

/// A soft spot: `inner` at its centre, `outer` from `radius` away on.
fn glow(ctx: &Ctx) -> Result<Image, TexError> {
    let (x, y) = (ctx.float("x"), ctx.float("y"));
    let (radius, power) = (ctx.float("radius"), ctx.float("power"));
    let (inner, outer) = (ctx.color("inner"), ctx.color("outer"));
    Ok(ctx.frame.fill(|u, v| {
        let (dx, dy) = (wrap_delta(u - x), wrap_delta(v - y));
        let d = (dx * dx + dy * dy).sqrt();
        let t = if radius > 0.0 { d / radius } else { 1.0 };
        mix(inner, outer, pow01(t.min(1.0), power))
    }))
}

pub(super) const OPS: &[OpSpec] = &[
    OpSpec {
        name: "flat",
        group: "Pattern",
        inputs: exactly(0),
        params: &[color("color", WHITE)],
        role: Role::Kernel,
        kernel: flat,
    },
    OpSpec {
        name: "noise",
        group: "Pattern",
        inputs: exactly(0),
        params: &[
            int("freq_x", 0, 256, 4),
            int("freq_y", 0, 256, 4),
            int("octaves", 1, 8, 4),
            float("persistence", 0.0, 1.0, 0.5),
            int("seed", 0, 65535, 1),
            choice("mode", &["fbm", "ridged"], 0),
            color("color0", BLACK),
            color("color1", WHITE),
        ],
        role: Role::Kernel,
        kernel: noise,
    },
    OpSpec {
        name: "checker",
        group: "Pattern",
        inputs: exactly(0),
        params: &[
            int("cols", 1, 64, 8),
            int("rows", 1, 64, 8),
            color("color0", BLACK),
            color("color1", WHITE),
        ],
        role: Role::Kernel,
        kernel: checker,
    },
    OpSpec {
        name: "stripes",
        group: "Pattern",
        inputs: exactly(0),
        params: &[
            int("cycles_x", -64, 64, 4),
            int("cycles_y", -64, 64, 4),
            float("duty", 0.0, 1.0, 0.5),
            color("color0", BLACK),
            color("color1", WHITE),
        ],
        role: Role::Kernel,
        kernel: stripes,
    },
    OpSpec {
        name: "gradient",
        group: "Pattern",
        inputs: exactly(0),
        params: &[
            int("cycles_x", -16, 16, 1),
            int("cycles_y", -16, 16, 0),
            color("color0", BLACK),
            color("color1", WHITE),
        ],
        role: Role::Kernel,
        kernel: gradient,
    },
    OpSpec {
        name: "glow",
        group: "Pattern",
        inputs: exactly(0),
        params: &[
            float("x", 0.0, 1.0, 0.5),
            float("y", 0.0, 1.0, 0.5),
            float("radius", 0.0, 1.0, 0.5),
            float("power", 0.1, 8.0, 1.0),
            color("inner", WHITE),
            color("outer", BLACK),
        ],
        role: Role::Kernel,
        kernel: glow,
    },
];
