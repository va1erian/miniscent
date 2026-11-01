//! Bump mapping: one normal fetch and one dot product per pixel.
//!
//! The baked light direction is already in the face tangent frame, so the
//! texture's tangent-space normal can be dotted with it directly, with no
//! per-pixel transform. The baked colour already contains the face-normal
//! Lambert term, so the bump term is the ratio of the perturbed and the flat
//! response; that keeps a flat normal map identical to no bump mapping.

use mscent_math::Vec3;

use crate::texture::Texture;

/// Lower bound on the flat response in the ratio, so grazing light (where the
/// baked colour is already small) cannot blow the ratio up.
const MIN_FLAT: f32 = 0.2;

/// Largest brightening the bump term may apply.
const MAX_GAIN: f32 = 2.0;

/// Decode a packed normal texel (`n * 0.5 + 0.5` per channel). Not
/// renormalized: the generated maps are unit length to 8-bit precision, and a
/// per-pixel square root is what this path exists to avoid.
pub fn decode_normal(packed: u32) -> Vec3 {
    let c = |shift: u32| f32::from(((packed >> shift) & 0xff) as u8) / 127.5 - 1.0;
    Vec3::new(c(16), c(8), c(0))
}

/// Tangent-space normal at a texel, or the flat normal without a map.
pub fn texel_normal(tex: &Texture, tx: i32, ty: i32) -> Vec3 {
    tex.normal_texel(tx, ty).map_or(FLAT_NORMAL, decode_normal)
}

/// The normal of an unperturbed surface in tangent space.
pub const FLAT_NORMAL: Vec3 = Vec3::new(0.0, 0.0, 1.0);

/// Light scale of normal `n` relative to the flat surface, for a unit light
/// direction `l` in tangent space. 1.0 for the flat normal, 0 when the
/// perturbed surface faces away from the light.
pub fn bump_gain(n: Vec3, l: Vec3) -> f32 {
    let flat = l.z.max(MIN_FLAT);
    (n.dot(l) / flat).clamp(0.0, MAX_GAIN)
}

#[cfg(test)]
#[path = "bump_tests.rs"]
mod bump_tests;
