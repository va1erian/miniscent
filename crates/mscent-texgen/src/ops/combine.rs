//! Operators that make one image out of several.

use super::{Ctx, Inputs, OpSpec, Role, choice};
use crate::error::TexError;
use crate::image::{Image, to_px, unit4};

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

pub(super) const OPS: &[OpSpec] = &[OpSpec {
    name: "merge",
    group: "Combine",
    inputs: Inputs { min: 2, max: 8 },
    params: &[choice("mode", MODES, 0)],
    role: Role::Kernel,
    kernel: merge,
}];
