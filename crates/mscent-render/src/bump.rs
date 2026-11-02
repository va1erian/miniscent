//! Bump mapping: one normal fetch and one dot product per pixel.
//!
//! The baked light direction is already in the face tangent frame, so the
//! texture's tangent-space normal can be dotted with it directly, with no
//! per-pixel transform. The baked colour already contains the face-normal
//! Lambert term, so the bump term is the ratio of the perturbed and the flat
//! response; that keeps a flat normal map identical to no bump mapping.

use mscent_math::Vec3;

use crate::texture::{NormalView, Texture};

/// Lower bound on the flat response in the ratio, so grazing light (where the
/// baked colour is already small) cannot blow the ratio up.
const MIN_FLAT: f32 = 0.2;

/// Darkest the bump term may make a texel: a pit still catches ambient light,
/// so mortar never goes pure black.
const MIN_GAIN: f32 = 0.12;

/// Largest brightening the bump term may apply.
const MAX_GAIN: f32 = 1.9;

/// Exaggeration of the normal map's tilt: the generated maps are subtle
/// next to the baked light, so the relief is scaled up (1.0 flat stays flat).
const RELIEF: f32 = 1.4;

/// Decode a packed normal texel (`n * 0.5 + 0.5` per channel). Not
/// renormalized: the generated maps are unit length to 8-bit precision, and a
/// per-pixel square root is what this path exists to avoid.
pub fn decode_normal(packed: u32) -> Vec3 {
    let c = |shift: u32| f32::from(((packed >> shift) & 0xff) as u8) / 127.5 - 1.0;
    Vec3::new(c(16), c(8), c(0))
}

/// Tangent-space normal at 16.16 fixed-point texel coordinates, bilinearly
/// filtered (texel centres at half-integers) so magnified bump maps show
/// smooth relief instead of texel stair-steps. The flat normal without a map.
pub fn texel_normal(tex: &Texture, fu: i32, fv: i32) -> Vec3 {
    tex.normal_view()
        .map_or(FLAT_NORMAL, |map| view_normal(&map, fu, fv))
}

/// [`texel_normal`] on a map already looked up, for the per-pixel loop.
#[inline]
pub fn view_normal(map: &NormalView, fu: i32, fv: i32) -> Vec3 {
    NormalSample::fetch(map, fu, fv).normal()
}

/// The four normal texels around a sample and their weights: fetched once for
/// the normal, and the specular mask in their alpha is there for no extra fetch,
/// to be blended only for the pixels inside a highlight.
#[derive(Clone, Copy)]
pub struct NormalSample {
    texels: [u32; 4],
    weights: [u32; 4],
}

impl NormalSample {
    #[inline]
    pub fn fetch(map: &NormalView, fu: i32, fv: i32) -> Self {
        let (u, v) = (fu.wrapping_sub(0x8000), fv.wrapping_sub(0x8000));
        let (x0, y0) = (u >> 16, v >> 16);
        let (tx, ty) = (((u >> 8) & 0xff) as u32, ((v >> 8) & 0xff) as u32);
        let (x1, y1) = (x0.wrapping_add(1), y0.wrapping_add(1));
        Self {
            texels: [
                map.texel(x0, y0),
                map.texel(x1, y0),
                map.texel(x0, y1),
                map.texel(x1, y1),
            ],
            // Exact integer weights, summing to 1 << 16.
            weights: [
                (256 - tx) * (256 - ty),
                tx * (256 - ty),
                (256 - tx) * ty,
                tx * ty,
            ],
        }
    }

    /// One byte channel of the texels, blended: 0 to `255 << 16`.
    #[inline]
    fn blend(&self, shift: u32) -> u32 {
        let byte = |i: usize| (self.texels[i] >> shift) & 0xff;
        byte(0) * self.weights[0]
            + byte(1) * self.weights[1]
            + byte(2) * self.weights[2]
            + byte(3) * self.weights[3]
    }

    /// The normal, bilinearly filtered: blends the bytes and converts once per
    /// channel, instead of decoding four texels to floats.
    #[inline]
    pub fn normal(&self) -> Vec3 {
        let ch = |shift: u32| self.blend(shift) as f32 * (1.0 / (65536.0 * 127.5)) - 1.0;
        Vec3::new(ch(16), ch(8), ch(0))
    }

    /// The specular mask, bilinearly filtered, 0.0 to 1.0. A division, not a
    /// reciprocal: four full texels must give exactly 1.0.
    #[inline]
    pub fn mask(&self) -> f32 {
        self.blend(24) as f32 / (65536.0 * 255.0)
    }
}

/// The normal of an unperturbed surface in tangent space.
pub const FLAT_NORMAL: Vec3 = Vec3::new(0.0, 0.0, 1.0);

/// Light scale of normal `n` relative to the flat surface, for a unit light
/// direction `l` in tangent space. 1.0 for the flat normal, [`MIN_GAIN`] when
/// the perturbed surface faces away from the light.
pub fn bump_gain(n: Vec3, l: Vec3) -> f32 {
    let flat = l.z.max(MIN_FLAT);
    // The flat normal must give exactly 1 even where the light is grazing
    // (z below MIN_FLAT), so the z term uses the same floored value.
    let tilted = n.x * RELIEF * l.x + n.y * RELIEF * l.y + n.z * flat;
    (tilted / flat).clamp(MIN_GAIN, MAX_GAIN)
}

#[cfg(test)]
#[path = "bump_tests.rs"]
mod bump_tests;
