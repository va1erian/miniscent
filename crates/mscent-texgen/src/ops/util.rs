//! What several kernels share: colour blending, a stateless hash, anti-aliased
//! coverage and the "draw over the input, or over a background" step.

use super::Ctx;
use crate::image::{Image, to_px, unit4};

pub(super) const BLACK: [f32; 4] = [0.0, 0.0, 0.0, 1.0];
pub(super) const WHITE: [f32; 4] = [1.0, 1.0, 1.0, 1.0];

/// Blends two colours; `t` is not clamped, the store clamps.
pub(super) fn mix(a: [f32; 4], b: [f32; 4], t: f32) -> [f32; 4] {
    core::array::from_fn(|i| a[i] + (b[i] - a[i]) * t)
}

/// A grey of full alpha.
pub(super) fn grey(v: f32) -> [f32; 4] {
    [v, v, v, 1.0]
}

/// A hash of a lattice point and a seed, so a texel can ask for its neighbours'
/// randomness without any table. Callers pass wrapped indices, which is what
/// makes the pattern repeat.
pub(super) fn hash(a: i32, b: i32, seed: u32) -> u32 {
    let mut h = (a as u32)
        .wrapping_mul(0x9E37_79B1)
        .wrapping_add((b as u32).wrapping_mul(0x85EB_CA77))
        .wrapping_add(seed.wrapping_mul(0xC2B2_AE3D));
    h ^= h >> 15;
    h = h.wrapping_mul(0x2767_79B1);
    h ^= h >> 13;
    h = h.wrapping_mul(0x1656_67B1);
    h ^ (h >> 16)
}

/// [`hash`] as a float in `0.0..1.0`; 24 bits, so every value is exact.
pub(super) fn unit_hash(a: i32, b: i32, seed: u32) -> f32 {
    (hash(a, b, seed) >> 8) as f32 / 16_777_216.0
}

/// The size of one texel in tile units, for the width of an anti-aliased edge.
/// The smaller side sets it, so an edge is never sharper than a texel.
pub(super) fn texel(ctx: &Ctx) -> f32 {
    1.0 / ctx.frame.size.width().min(ctx.frame.size.height()) as f32
}

/// Coverage `0.0..=1.0` of a shape at signed distance `sd` (negative inside)
/// from its edge: a ramp of `softness` plus one texel, centred on the edge.
pub(super) fn coverage(sd: f32, softness: f32, texel: f32) -> f32 {
    let width = (softness + texel).max(1e-6);
    (0.5 - sd / width).clamp(0.0, 1.0)
}

/// `color` laid over `base` by `cover` times its alpha; alpha accumulates as
/// "over" does.
pub(super) fn over(base: [f32; 4], color: [f32; 4], cover: f32) -> [f32; 4] {
    let a = cover * color[3];
    [
        base[0] + (color[0] - base[0]) * a,
        base[1] + (color[1] - base[1]) * a,
        base[2] + (color[2] - base[2]) * a,
        base[3] + (1.0 - base[3]) * a,
    ]
}

/// What a drawing operator draws on: its input where it has one, else the
/// `background` colour.
pub(super) fn canvas_at(ctx: &Ctx, x: i32, y: i32) -> [f32; 4] {
    match ctx.inputs.first() {
        Some(input) => unit4(input.at(x, y)),
        None => ctx.color("background"),
    }
}

/// A drawing operator whose shape is a coverage function of normalized
/// coordinates, laid in `color` over the canvas.
pub(super) fn draw(ctx: &Ctx, cover: impl Fn(f32, f32) -> f32) -> Image {
    let color = ctx.color("color");
    Image::from_xy(ctx.frame.size, |x, y| {
        let (u, v) = ctx.frame.uv(x, y);
        to_px(over(canvas_at(ctx, x, y), color, cover(u, v)))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_is_stable_and_spreads() {
        assert_eq!(hash(3, 4, 5), hash(3, 4, 5));
        assert_ne!(hash(3, 4, 5), hash(4, 3, 5));
        assert_ne!(hash(3, 4, 5), hash(3, 4, 6));
        let mean = (0..1000).map(|i| unit_hash(i, 7, 1)).sum::<f32>() / 1000.0;
        assert!((mean - 0.5).abs() < 0.05, "{mean}");
        assert!((0..1000).all(|i| (0.0..1.0).contains(&unit_hash(i, -i, 9))));
    }

    #[test]
    fn coverage_is_half_on_the_edge_and_saturates() {
        assert_eq!(coverage(0.0, 0.0, 0.1), 0.5);
        assert_eq!(coverage(-1.0, 0.0, 0.1), 1.0);
        assert_eq!(coverage(1.0, 0.0, 0.1), 0.0);
        assert_eq!(coverage(0.0, 0.0, 0.0), 0.5);
    }

    #[test]
    fn over_is_the_usual_compositing() {
        let c = over([0.0, 0.0, 0.0, 0.5], [1.0, 1.0, 1.0, 1.0], 0.5);
        assert_eq!(c, [0.5, 0.5, 0.5, 0.75]);
    }
}
