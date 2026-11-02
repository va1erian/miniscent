//! Tools that read an image as a height (its red channel) and turn it into light
//! or a mask. Differences are wrapped, so the result tiles with the input.
//!
//! Strength is defined at 128 px, as the plan sets for `normal_strength`: a
//! difference between neighbouring texels shrinks as the tile grows, so it is
//! multiplied by `size / 128` and a recipe looks the same at every size.

use super::cost::Cost;
use super::filter::box_blur;
use super::util::grey;
use super::{Ctx, OpSpec, Role, exactly, float};
use crate::error::TexError;
use crate::image::{Image, to_px, unit, unit4};

/// Height of a texel, `0.0..=1.0`.
fn height_at(image: &Image, x: i32, y: i32) -> f32 {
    unit(image.at(x, y)[0])
}

/// Slope across the tile at a texel by central differences, `strength` already
/// in. The wrap is `Image::at`'s.
fn slope_at(image: &Image, x: i32, y: i32, strength: f32) -> (f32, f32) {
    let size = image.size();
    let gx = height_at(image, x + 1, y) - height_at(image, x - 1, y);
    let gy = height_at(image, x, y + 1) - height_at(image, x, y - 1);
    (
        gx * 0.5 * strength * size.width() as f32 / 128.0,
        gy * 0.5 * strength * size.height() as f32 / 128.0,
    )
}

/// Lights the albedo from a height. The light is a direction towards it,
/// `(light_x, light_y, 1)`; a flat height is lit by exactly 1, so it leaves the
/// albedo as it was and only relief changes it. `ambient` is the share of the
/// albedo that stays whatever the light does.
fn bump(ctx: &Ctx) -> Result<Image, TexError> {
    let (albedo, height) = (ctx.input(0)?, ctx.input(1)?);
    let (lx, ly) = (ctx.float("light_x"), ctx.float("light_y"));
    let (strength, ambient) = (ctx.float("strength"), ctx.float("ambient"));
    Ok(Image::from_xy(albedo.size(), |x, y| {
        let (gx, gy) = slope_at(height, x, y, strength);
        // The surface normal is (-gx, -gy, 1) over its length.
        let shade = ((1.0 - gx * lx - gy * ly) / (1.0 + gx * gx + gy * gy).sqrt()).max(0.0);
        let lit = ambient + (1.0 - ambient) * shade;
        let c = unit4(albedo.at(x, y));
        to_px([c[0] * lit, c[1] * lit, c[2] * lit, c[3]])
    }))
}

/// Grey that darkens where the height is below its blurred self: crevices.
fn cavity(ctx: &Ctx) -> Result<Image, TexError> {
    let height = ctx.input(0)?;
    let soft = box_blur(height, ctx.float("radius"));
    let amount = ctx.float("amount");
    Ok(Image::from_xy(height.size(), |x, y| {
        let deficit = (height_at(&soft, x, y) - height_at(height, x, y)).max(0.0);
        to_px(grey(1.0 - amount * deficit))
    }))
}

/// Grey from how steep the height is.
fn slope(ctx: &Ctx) -> Result<Image, TexError> {
    let height = ctx.input(0)?;
    let strength = ctx.float("strength");
    Ok(Image::from_xy(height.size(), |x, y| {
        let (gx, gy) = slope_at(height, x, y, strength);
        to_px(grey((gx * gx + gy * gy).sqrt()))
    }))
}

pub(super) const OPS: &[OpSpec] = &[
    OpSpec {
        name: "bump",
        group: "Height",
        inputs: exactly(2),
        params: &[
            float("light_x", -1.0, 1.0, -0.5),
            float("light_y", -1.0, 1.0, -0.5),
            float("strength", 0.0, 32.0, 4.0),
            float("ambient", 0.0, 1.0, 0.5),
        ],
        role: Role::Kernel,
        kernel: bump,
        cost: Cost::Flat(21.0),
    },
    OpSpec {
        name: "cavity",
        group: "Height",
        inputs: exactly(1),
        params: &[
            float("radius", 0.0, 0.25, 0.05),
            float("amount", 0.0, 8.0, 2.0),
        ],
        role: Role::Kernel,
        kernel: cavity,
        cost: Cost::Flat(140.0),
    },
    OpSpec {
        name: "slope",
        group: "Height",
        inputs: exactly(1),
        params: &[float("strength", 0.0, 32.0, 4.0)],
        role: Role::Kernel,
        kernel: slope,
        cost: Cost::Flat(27.0),
    },
];
