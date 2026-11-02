//! Operators that change one image, texel by texel or by a wrapped blur.

use super::util::WHITE;
use super::{Ctx, OpSpec, Role, color, exactly, float};
use crate::error::TexError;
use crate::image::{Image, Px, to_px, unit4};
use crate::math::{pow01, sin_cos};

/// One wrapped box pass along a row or column, by running sum, so its cost does
/// not grow with the radius. `step` is 1 along x and the row length along y.
fn box_line(src: &[Px], dst: &mut [Px], start: usize, count: usize, step: usize, radius: usize) {
    let at = |i: usize| src[start + (i % count) * step];
    let width = (2 * radius + 1) as u64;
    let mut sum = [0u64; 4];
    for i in 0..width as usize {
        let p = at(i + count - (radius % count));
        for c in 0..4 {
            sum[c] += u64::from(p[c]);
        }
    }
    for i in 0..count {
        dst[start + i * step] = core::array::from_fn(|c| ((sum[c] + width / 2) / width) as u16);
        let (leaving, entering) = (at(i + count - (radius % count)), at(i + radius + 1));
        for c in 0..4 {
            sum[c] = sum[c] - u64::from(leaving[c]) + u64::from(entering[c]);
        }
    }
}

/// Three box passes per axis, close to a Gaussian. The radius is a fraction of
/// each side, so the picture does not change with the size.
pub(super) fn box_blur(source: &Image, radius: f32) -> Image {
    let size = source.size();
    let (w, h) = (size.width() as usize, size.height() as usize);
    let (rx, ry) = (
        (radius * w as f32 + 0.5) as usize,
        (radius * h as f32 + 0.5) as usize,
    );
    let mut a = source.pixels().to_vec();
    let mut b = a.clone();
    for _ in 0..3 {
        for y in 0..h {
            box_line(&a, &mut b, y * w, w, 1, rx);
        }
        for x in 0..w {
            box_line(&b, &mut a, x, h, w, ry);
        }
    }
    let mut pixels = a.iter().copied();
    Image::from_xy(size, |_, _| pixels.next().unwrap_or([0; 4]))
}

fn blur(ctx: &Ctx) -> Result<Image, TexError> {
    Ok(box_blur(ctx.input(0)?, ctx.float("radius")))
}

/// Runs `f` on each texel's colour, as floats; alpha is passed through, so a
/// filter cannot make a texture see-through by accident.
fn per_texel(ctx: &Ctx, f: impl Fn([f32; 3]) -> [f32; 3]) -> Result<Image, TexError> {
    Ok(ctx.input(0)?.map(|p| {
        let c = unit4(p);
        let [r, g, b] = f([c[0], c[1], c[2]]);
        to_px([r, g, b, c[3]])
    }))
}

/// Multiplies by a colour and an amount, which may take values past white: a
/// later filter can bring them back.
fn color_gain(ctx: &Ctx) -> Result<Image, TexError> {
    let (gain, amount) = (ctx.color("gain"), ctx.float("amount"));
    Ok(ctx.input(0)?.map(|p| {
        let c = unit4(p);
        to_px([
            c[0] * gain[0] * amount,
            c[1] * gain[1] * amount,
            c[2] * gain[2] * amount,
            c[3] * gain[3],
        ])
    }))
}

/// Levels: map `in_low..in_high` to `out_low..out_high` with a gamma. An empty
/// or reversed input range is a threshold at `in_low` instead of a division.
fn range(ctx: &Ctx) -> Result<Image, TexError> {
    let (in_low, in_high) = (ctx.float("in_low"), ctx.float("in_high"));
    let (out_low, out_high) = (ctx.float("out_low"), ctx.float("out_high"));
    let inverse_gamma = 1.0 / ctx.float("gamma");
    let level = |c: f32| {
        let t = if in_high > in_low {
            ((c - in_low) / (in_high - in_low)).clamp(0.0, 1.0)
        } else {
            f32::from(u8::from(c >= in_low))
        };
        out_low + (out_high - out_low) * pow01(t, inverse_gamma)
    };
    per_texel(ctx, |[r, g, b]| [level(r), level(g), level(b)])
}

fn invert(ctx: &Ctx) -> Result<Image, TexError> {
    per_texel(ctx, |[r, g, b]| [1.0 - r, 1.0 - g, 1.0 - b])
}

/// Rotation of the colour cube about its grey axis by `turns`, in plain RGB
/// arithmetic.
fn rotate_hue([r, g, b]: [f32; 3], turns: f32) -> [f32; 3] {
    let (s, c) = sin_cos(turns * core::f32::consts::TAU);
    let k = (1.0 - c) / 3.0;
    // sqrt(1/3)
    let q = s * 0.577_350_26;
    [
        r * (c + k) + g * (k - q) + b * (k + q),
        r * (k + q) + g * (c + k) + b * (k - q),
        r * (k - q) + g * (k + q) + b * (c + k),
    ]
}

/// Hue, saturation, contrast and brightness, in that order. The defaults leave
/// the picture as it was.
fn hscb(ctx: &Ctx) -> Result<Image, TexError> {
    let (hue, saturation) = (ctx.float("hue"), ctx.float("saturation"));
    let (contrast, brightness) = (ctx.float("contrast"), ctx.float("brightness"));
    per_texel(ctx, |rgb| {
        let turned = rotate_hue(rgb, hue);
        let luma = 0.299 * turned[0] + 0.587 * turned[1] + 0.114 * turned[2];
        turned.map(|c| ((luma + (c - luma) * saturation - 0.5) * contrast + 0.5) * brightness)
    })
}

/// Unsharp mask: add back what a blur takes away.
fn sharpen(ctx: &Ctx) -> Result<Image, TexError> {
    let source = ctx.input(0)?;
    let soft = box_blur(source, ctx.float("radius"));
    let amount = ctx.float("amount");
    Ok(Image::from_xy(source.size(), |x, y| {
        let (c, s) = (unit4(source.at(x, y)), unit4(soft.at(x, y)));
        to_px([
            c[0] + (c[0] - s[0]) * amount,
            c[1] + (c[1] - s[1]) * amount,
            c[2] + (c[2] - s[2]) * amount,
            c[3],
        ])
    }))
}

pub(super) const OPS: &[OpSpec] = &[
    OpSpec {
        name: "blur",
        group: "Filter",
        inputs: exactly(1),
        params: &[float("radius", 0.0, 0.25, 0.02)],
        role: Role::Kernel,
        kernel: blur,
    },
    OpSpec {
        name: "color",
        group: "Filter",
        inputs: exactly(1),
        params: &[color("gain", WHITE), float("amount", 0.0, 8.0, 1.0)],
        role: Role::Kernel,
        kernel: color_gain,
    },
    OpSpec {
        name: "range",
        group: "Filter",
        inputs: exactly(1),
        params: &[
            float("in_low", 0.0, 1.0, 0.0),
            float("in_high", 0.0, 1.0, 1.0),
            float("out_low", 0.0, 1.0, 0.0),
            float("out_high", 0.0, 1.0, 1.0),
            float("gamma", 0.1, 8.0, 1.0),
        ],
        role: Role::Kernel,
        kernel: range,
    },
    OpSpec {
        name: "invert",
        group: "Filter",
        inputs: exactly(1),
        params: &[],
        role: Role::Kernel,
        kernel: invert,
    },
    OpSpec {
        name: "hscb",
        group: "Filter",
        inputs: exactly(1),
        params: &[
            float("hue", -1.0, 1.0, 0.0),
            float("saturation", 0.0, 4.0, 1.0),
            float("contrast", 0.0, 4.0, 1.0),
            float("brightness", 0.0, 4.0, 1.0),
        ],
        role: Role::Kernel,
        kernel: hscb,
    },
    OpSpec {
        name: "sharpen",
        group: "Filter",
        inputs: exactly(1),
        params: &[
            float("amount", 0.0, 4.0, 1.0),
            float("radius", 0.0, 0.1, 0.01),
        ],
        role: Role::Kernel,
        kernel: sharpen,
    },
];
