//! Specular highlights, a High quality option.
//!
//! Blinn-Phong: the half vector is built from the baked light direction and
//! the view direction at the two span ends and stepped linearly, so the
//! per-pixel cost is one dot product and one table lookup. The power
//! function is a 256-entry table per gloss value; materials share tables.

use mscent_math::Vec3;

/// Entries in the power table; index is `n.h` scaled to `0..=255`.
const TABLE_LEN: usize = 256;

/// Specular response `(n.h)^power * gloss`, tabulated.
#[derive(Clone, Debug)]
pub struct SpecTable {
    lut: [f32; TABLE_LEN],
    strength: f32,
}

/// Peak response below which a highlight is not worth its per-pixel cost:
/// at 0.12 a bright pool (light level ~300 of 256) gains about 40 levels at
/// the very peak of the lobe, and less everywhere else.
const MIN_VISIBLE: f32 = 0.12;

impl SpecTable {
    /// Table for a material gloss of 0 (matte, no highlight) to 255
    /// (tight, full-strength highlight).
    pub fn new(gloss: u8) -> Self {
        let g = f32::from(gloss);
        // Tighter highlights and stronger ones both grow with gloss.
        let power = 6.0 + g * 0.1;
        let strength = g / 255.0;
        let mut lut = [0.0; TABLE_LEN];
        for (i, e) in lut.iter_mut().enumerate() {
            *e = (i as f32 / (TABLE_LEN - 1) as f32).powf(power) * strength;
        }
        Self { lut, strength }
    }

    /// Whether the material's highlight is strong enough to draw. Matte
    /// materials (rock, most brick) skip the specular path entirely.
    pub fn is_visible(&self) -> bool {
        self.strength >= MIN_VISIBLE
    }

    /// Response for a dot product `n.h`; non-positive dots give 0.
    pub fn response(&self, n_dot_h: f32) -> f32 {
        // NaN clamps to NaN and casts to 0, so it reads the first entry.
        let x = n_dot_h.clamp(0.0, 1.0) * (TABLE_LEN - 1) as f32;
        let i = (x as usize).min(TABLE_LEN - 2);
        let t = x - i as f32;
        // Interpolate: stepping between entries shows up as bright rings.
        self.lut[i] + (self.lut[i + 1] - self.lut[i]) * t
    }
}

/// Unit half vector between the light direction `l` and the direction `v`
/// towards the eye. Falls back to `l` when they cancel out.
pub fn half_vector(l: Vec3, v: Vec3) -> Vec3 {
    (l + v).normalized().unwrap_or(l)
}

#[cfg(test)]
#[path = "specular_tests.rs"]
mod specular_tests;
