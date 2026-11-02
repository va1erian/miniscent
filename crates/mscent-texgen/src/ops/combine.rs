//! Operators that make one image out of several.

use super::util::mix;
use super::{Ctx, Inputs, OpSpec, Role, choice, exactly, float};
use crate::error::TexError;
use crate::image::{Image, to_px, unit4};
use crate::math::wrap_delta;

const MODES: &[&str] = &[
    "add", "subtract", "multiply", "min", "max", "screen", "blend",
];

/// One channel of `a` merged with `b`. `blend` is for whole texels, below.
fn merged(mode: u8, a: f32, b: f32) -> f32 {
    match mode {
        0 => a + b,
        1 => a - b,
        2 => a * b,
        3 => a.min(b),
        4 => a.max(b),
        _ => 1.0 - (1.0 - a) * (1.0 - b),
    }
}

/// Folds the inputs left to right. Colour and alpha merge alike, except `blend`,
/// which lays each input over the last by that input's alpha.
fn merge(ctx: &Ctx) -> Result<Image, TexError> {
    let mode = ctx.choice("mode");
    let mut out = ctx.input(0)?.clone();
    for &layer in ctx.inputs.iter().skip(1) {
        out = Image::from_xy(out.size(), |x, y| {
            let (a, b) = (unit4(out.at(x, y)), unit4(layer.at(x, y)));
            to_px(if mode == 6 {
                let t = b[3];
                [
                    a[0] + (b[0] - a[0]) * t,
                    a[1] + (b[1] - a[1]) * t,
                    a[2] + (b[2] - a[2]) * t,
                    a[3].max(t),
                ]
            } else {
                core::array::from_fn(|i| merged(mode, a[i], b[i]))
            })
        });
    }
    Ok(out)
}

/// Blends `a` into `b` texel by texel by the red of the mask.
fn mask(ctx: &Ctx) -> Result<Image, TexError> {
    let (a, b, m) = (ctx.input(0)?, ctx.input(1)?, ctx.input(2)?);
    Ok(Image::from_xy(a.size(), |x, y| {
        let t = unit4(m.at(x, y))[0];
        to_px(mix(unit4(a.at(x, y)), unit4(b.at(x, y)), t))
    }))
}

/// Puts the tile input, scaled into a rectangle centred on `x`, `y`, over the
/// background. The edge is hard and the rectangle wraps. The tile's alpha
/// decides how much of the background shows through.
///
/// The tile is read through the frame, like a warp's source, so that it stays
/// anchored to the picture when the window moves.
fn paste(ctx: &Ctx) -> Result<Image, TexError> {
    let (background, tile) = (ctx.input(0)?, ctx.input(1)?);
    let (x, y) = (ctx.float("x"), ctx.float("y"));
    let (w, h) = (ctx.float("w"), ctx.float("h"));
    Ok(Image::from_xy(ctx.frame.size, |px, py| {
        let (u, v) = ctx.frame.uv(px, py);
        let (dx, dy) = (wrap_delta(u - x) + w / 2.0, wrap_delta(v - y) + h / 2.0);
        let base = unit4(background.at(px, py));
        if !(0.0..w).contains(&dx) || !(0.0..h).contains(&dy) {
            return to_px(base);
        }
        let t = ctx.frame.sample(tile, dx / w, dy / h);
        let mut out = mix(base, t, t[3]);
        out[3] = base[3] + (1.0 - base[3]) * t[3];
        to_px(out)
    }))
}

pub(super) const OPS: &[OpSpec] = &[
    OpSpec {
        name: "merge",
        group: "Combine",
        inputs: Inputs { min: 2, max: 8 },
        params: &[choice("mode", MODES, 0)],
        role: Role::Kernel,
        kernel: merge,
    },
    OpSpec {
        name: "mask",
        group: "Combine",
        inputs: exactly(3),
        params: &[],
        role: Role::Kernel,
        kernel: mask,
    },
    OpSpec {
        name: "paste",
        group: "Combine",
        inputs: exactly(2),
        params: &[
            float("x", 0.0, 1.0, 0.5),
            float("y", 0.0, 1.0, 0.5),
            float("w", 0.0, 1.0, 0.5),
            float("h", 0.0, 1.0, 0.5),
        ],
        role: Role::Kernel,
        kernel: paste,
    },
];
