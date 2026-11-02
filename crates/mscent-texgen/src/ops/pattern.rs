//! Generators: operators that make a tile from nothing but their parameters.

use super::{Ctx, OpSpec, Role, choice, color, exactly, int};
use crate::error::TexError;
use crate::image::{Image, to_px};
use crate::noise::{Fold, Noise};
use crate::ops::float;

const BLACK: [f32; 4] = [0.0, 0.0, 0.0, 1.0];
const WHITE: [f32; 4] = [1.0, 1.0, 1.0, 1.0];

/// Blends two colours; `t` is not clamped, the store clamps.
fn mix(a: [f32; 4], b: [f32; 4], t: f32) -> [f32; 4] {
    core::array::from_fn(|i| a[i] + (b[i] - a[i]) * t)
}

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
];
