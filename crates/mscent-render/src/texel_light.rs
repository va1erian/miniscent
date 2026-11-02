//! A texel's final colour under a light: what the lighting shader does for each
//! pixel after it has found that pixel's light, and all the Texture Studio's lit
//! view needs to show a material as the game does.
//!
//! The shader calls the steps here one by one from its loop, since it skips the
//! normal fetch and blends the specular mask only where they matter. These are
//! inlined into it, so sharing them costs a frame nothing. [`shade_texel`] is the
//! same steps in one call for a caller with no such loop, and a test in
//! `lighting_shader_tests.rs` holds it to the shader's pixels on a real face.

use mscent_math::Vec3;

use crate::bump::{FLAT_NORMAL, bump_gain};
use crate::specular::SpecTable;

/// Highlight strength: the table peaks at gloss/255 of the light colour,
/// which on dim baked light would be invisible.
pub const SPEC_GAIN: f32 = 1.1;

/// Table response below which a highlight adds nothing visible (the light
/// level tops out near 300 of 256, so 1/300 is under one level).
pub const MIN_SPEC: f32 = 1.0 / 300.0;

/// Light level (256 = full) above which [`soft_limit`] starts rolling off.
pub const KNEE: f32 = 190.0;
/// Distance past the knee at which the roll-off reaches its ceiling.
pub const ROLL: f32 = 110.0;

/// Multiply the colour channels by per-channel light `k` (256 = unchanged).
/// Negative or NaN products saturate to zero in the float-to-int cast.
/// `dither` in `[0, 1)` is added before truncation so smooth gradients do not
/// quantise into visible rings.
#[inline(always)]
pub(crate) fn mul_rgb3(c: u32, k: Vec3, dither: f32) -> u32 {
    let ch =
        |shift: u32, k: f32| (((c >> shift) & 0xff) as f32 * k * (1.0 / 256.0) + dither) as u32;
    0xff00_0000 | ch(16, k.x).min(255) << 16 | ch(8, k.y).min(255) << 8 | ch(0, k.z).min(255)
}

/// Smoothly compresses light above the knee towards `KNEE + ROLL`, so bright
/// pools keep texture detail instead of clipping to flat white.
#[inline(always)]
pub(crate) fn soft_limit(x: f32) -> f32 {
    // KNEE + d / (1 + d / ROLL) with d = x - KNEE, rearranged to x - d^2 /
    // (ROLL + d) so there is no branch: d is zero below the knee.
    let d = (x - KNEE).max(0.0);
    x - d * d / (ROLL + d)
}

/// Rolls light off above the knee. Below it the roll-off is the identity, so its
/// divisions are skipped.
#[inline(always)]
pub(crate) fn limit_light(k: Vec3) -> Vec3 {
    if k.x.max(k.y).max(k.z) > KNEE {
        Vec3::new(soft_limit(k.x), soft_limit(k.y), soft_limit(k.z))
    } else {
        k
    }
}

/// Add a white highlight of strength `s` scaled by the light colour `k`,
/// saturating per channel.
#[inline(always)]
pub(crate) fn add_spec(c: u32, k: Vec3, s: f32) -> u32 {
    // The float cast saturates at u32::MAX, so the add must saturate too.
    let ch = |shift: u32, k: f32| (((c >> shift) & 0xff).saturating_add((k * s) as u32)).min(255);
    0xff00_0000 | ch(16, k.x) << 16 | ch(8, k.y) << 8 | ch(0, k.z)
}

/// The table's response at normal `n` and half vector `h`. A half vector stepped
/// linearly between unit vectors is short mid-interval, which would flatten the
/// peak; one Newton step of 1/|h| restores it.
#[inline(always)]
pub(crate) fn spec_response(table: &SpecTable, n: Vec3, h: Vec3) -> f32 {
    table.response(n.dot(h) * (1.5 - 0.5 * h.dot(h)))
}

/// Adds the highlight of response `s` to `out`. Most pixels are outside the lobe;
/// below [`MIN_SPEC`] the add is under one level for any light the shader can
/// produce, and the mask only lowers the response, so those pixels need not blend
/// it, which is why it comes as a function.
#[inline(always)]
pub(crate) fn add_highlight(out: u32, k: Vec3, s: f32, mask: impl FnOnce() -> f32) -> u32 {
    if s > MIN_SPEC {
        add_spec(out, k, s * mask() * SPEC_GAIN)
    } else {
        out
    }
}

/// Cheap noise in `[0, 1)` from the screen position. It is fixed on the
/// screen, so it cannot crawl across surfaces as the camera moves (noise
/// keyed by span index or texel does, since both shift every frame).
#[inline(always)]
pub fn dither_of(x: i32, y: i32) -> f32 {
    let h = (x as u32).wrapping_mul(0x9E37_79B1) ^ (y as u32).wrapping_mul(0x85EB_CA6B);
    let h = (h ^ (h >> 15)).wrapping_mul(0x2C1B_3C6D);
    ((h >> 8) & 0xffff) as f32 * (1.0 / 65536.0)
}

/// The light arriving at one texel, in the texture frame.
#[derive(Clone, Copy, Debug)]
pub struct TexelLight {
    /// Colour, 256 = full, with the face's Lambert term already in it.
    pub color: Vec3,
    /// Towards the light.
    pub dir: Vec3,
    /// Halfway between `dir` and the direction towards the eye.
    pub half: Vec3,
}

/// What a material adds to a texel beyond its albedo.
#[derive(Clone, Copy, Debug)]
pub struct TexelSurface<'a> {
    /// The tangent-space normal; `None` for no normal map, or bump mapping off.
    pub normal: Option<Vec3>,
    /// The power table, when the material draws a highlight at all.
    pub table: Option<&'a SpecTable>,
    /// The specular mask here, 0.0 to 1.0.
    pub mask: f32,
}

/// A texel's final colour, as the lighting shader computes it. `dither` as in
/// [`mul_rgb3`], which is to say [`dither_of`] the pixel's screen position.
pub fn shade_texel(texel: u32, light: TexelLight, surface: TexelSurface, dither: f32) -> u32 {
    let mut k = light.color;
    let n = surface.normal.unwrap_or(FLAT_NORMAL);
    if surface.normal.is_some() {
        k = k * bump_gain(n, light.dir);
    }
    let k = limit_light(k);
    let out = mul_rgb3(texel, k, dither);
    match surface.table {
        Some(table) => add_highlight(out, k, spec_response(table, n, light.half), || surface.mask),
        None => out,
    }
}
