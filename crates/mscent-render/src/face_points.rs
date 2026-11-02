//! Point lights as one face sees them: the light of up to
//! [`MAX_DYN_LIGHTS`] short-lived lights, added to the baked light of a span
//! by [`FacePoints::add_span`].
//!
//! Each light is kept in the face's tangent frame, where the face is the
//! plane `z = 0` and the light's `z` is its height above the face, so the
//! `N.L` of a point is `z / distance` with no per-point dot product. The
//! light is evaluated at the two ends of a span and stepped between them,
//! like the fog.

use mscent_math::Vec3;
use mscent_math::epsilon::MIN_LENGTH_F32;
use mscent_world::lightmap::FaceProjection;

use crate::dynlight::MAX_DYN_LIGHTS;
use crate::lightmap_mips::Rgbx;

/// Light scale of the shader, where 256 is the plain texture colour.
const FULL: f32 = 256.0;

#[derive(Clone, Copy)]
struct Term {
    /// Light position in the tangent frame.
    pos: Vec3,
    radius: f32,
    /// Colour in shader units (256 = full light).
    color: Vec3,
}

/// The lights acting on one face, empty for almost every face of a frame.
#[derive(Clone, Copy)]
pub struct FacePoints {
    terms: [Term; MAX_DYN_LIGHTS],
    count: usize,
    /// Tangent-frame position of luxel (0, 0), and the luxel size.
    origin: (f32, f32),
    luxel: f32,
}

impl FacePoints {
    pub const NONE: Self = Self {
        terms: [Term {
            pos: Vec3::ZERO,
            radius: 0.0,
            color: Vec3::ZERO,
        }; MAX_DYN_LIGHTS],
        count: 0,
        origin: (0.0, 0.0),
        luxel: 0.0,
    };

    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// Adds a light at world `pos` for the face `proj`. False if the
    /// light does not count: all slots are taken, or a value is not finite or
    /// not positive.
    pub fn add(&mut self, proj: &FaceProjection, pos: Vec3, radius: f32, color: Vec3) -> bool {
        let ok = pos.is_finite() && color.is_finite() && radius.is_finite() && radius > 0.0;
        let Some(term) = self.terms.get_mut(self.count).filter(|_| ok) else {
            return false;
        };
        *term = Term {
            pos: proj.to_tangent(pos - proj.origin),
            radius,
            color: color * FULL,
        };
        self.count += 1;
        self.origin = (proj.u_min, proj.v_min);
        self.luxel = proj.luxel_size;
        true
    }

    /// Light at luxel coordinates `(lx, ly)`, in shader units.
    fn at(&self, lx: f32, ly: f32) -> Vec3 {
        let (x, y) = (
            self.origin.0 + lx * self.luxel,
            self.origin.1 + ly * self.luxel,
        );
        let mut sum = Vec3::ZERO;
        for t in &self.terms[..self.count] {
            let to = Vec3::new(t.pos.x - x, t.pos.y - y, t.pos.z);
            let dist = to.length();
            // A NaN distance fails both tests and adds nothing.
            if dist < t.radius && dist > MIN_LENGTH_F32 {
                let fall = 1.0 - dist / t.radius;
                // Behind the face the light is zero, not negative.
                sum = sum + t.color * ((to.z / dist).max(0.0) * fall * fall);
            }
        }
        sum
    }

    /// Adds the lights to `col`, the colours of a span of `len` pixels whose
    /// ends are at luxel coordinates `a` and `b`.
    pub fn add_span(&self, col: &mut [Rgbx], a: (f32, f32), b: (f32, f32), len: u32) {
        let (c0, c1) = (self.at(a.0, a.1), self.at(b.0, b.1));
        if c0 == Vec3::ZERO && c1 == Vec3::ZERO {
            return;
        }
        let step = (c1 - c0) * (1.0 / len.max(1) as f32);
        for (i, px) in col.iter_mut().enumerate() {
            let k = c0 + step * i as f32;
            px[0] += k.x;
            px[1] += k.y;
            px[2] += k.z;
        }
    }
}

#[cfg(test)]
#[path = "face_points_tests.rs"]
mod face_points_tests;
