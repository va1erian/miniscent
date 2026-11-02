//! Operators that change one image.

use super::{Ctx, OpSpec, Role, exactly, float};
use crate::error::TexError;
use crate::image::{Image, Px};

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
fn blur(ctx: &Ctx) -> Result<Image, TexError> {
    let source = ctx.input(0)?;
    let size = source.size();
    let (w, h) = (size.width() as usize, size.height() as usize);
    let (rx, ry) = (
        (ctx.float("radius") * w as f32 + 0.5) as usize,
        (ctx.float("radius") * h as f32 + 0.5) as usize,
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
    Ok(Image::from_xy(size, |_, _| pixels.next().unwrap_or([0; 4])))
}

pub(super) const OPS: &[OpSpec] = &[OpSpec {
    name: "blur",
    group: "Filter",
    inputs: exactly(1),
    params: &[float("radius", 0.0, 0.25, 0.02)],
    role: Role::Kernel,
    kernel: blur,
}];
