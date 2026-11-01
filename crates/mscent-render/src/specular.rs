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
}

impl SpecTable {
    /// Table for a material gloss of 0 (matte, no highlight) to 255
    /// (tight, full-strength highlight).
    pub fn new(gloss: u8) -> Self {
        let g = f32::from(gloss);
        // Tighter highlights and stronger ones both grow with gloss.
        let power = 5.0 + g * 0.28;
        let strength = g / 255.0;
        let mut lut = [0.0; TABLE_LEN];
        for (i, e) in lut.iter_mut().enumerate() {
            *e = (i as f32 / (TABLE_LEN - 1) as f32).powf(power) * strength;
        }
        Self { lut }
    }

    /// Response for a dot product `n.h`; non-positive dots give 0.
    pub fn response(&self, n_dot_h: f32) -> f32 {
        let i = (n_dot_h.clamp(0.0, 1.0) * (TABLE_LEN - 1) as f32) as usize;
        self.lut[i.min(TABLE_LEN - 1)]
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
