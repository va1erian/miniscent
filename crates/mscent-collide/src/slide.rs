//! Slide response (M4.2): move a sphere along a velocity, deflecting it along
//! every surface it meets, with a bounded number of bumps.
//!
//! Each bump sweeps the remaining time, then re-derives the velocity from the
//! ORIGINAL velocity and all planes touched so far. Re-deriving from the
//! original (rather than clipping the already clipped velocity) is what keeps
//! corners from jittering: the result depends only on the set of planes.

use crate::sweep::{Solids, sweep_sphere_d};
use crate::tuning::{CLIP_SLACK, MIN_SPEED};
use mscent_math::{Vec3, Vec3d};

/// Most surfaces one move may bounce off. Three planes already pin a sphere
/// in a corner; the fourth bump is the spare for numerical coincidences.
pub const MAX_BUMPS: usize = 4;

/// Two normals closer than this (cosine) are the same surface.
const SAME_PLANE: f64 = 0.999;

/// Result of [`slide_move`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SlideResult {
    /// Final centre.
    pub pos: Vec3,
    /// Velocity after deflection; what the mover keeps for the next step.
    pub vel: Vec3,
    /// Distinct surface normals touched, first `contacts` entries valid.
    pub normals: [Vec3; MAX_BUMPS],
    pub contacts: usize,
}

impl SlideResult {
    /// The surface normals touched during the move.
    pub fn touched(&self) -> &[Vec3] {
        &self.normals[..self.contacts]
    }
}

/// Moves a sphere of `radius` from `pos` with velocity `vel` for `dt`
/// seconds, sliding along whatever it hits. Never panics on bad input: a
/// non-finite state leaves the sphere where it is with zero velocity.
pub fn slide_move<'a>(
    solids: impl Into<Solids<'a>>,
    pos: Vec3,
    vel: Vec3,
    dt: f32,
    radius: f32,
) -> SlideResult {
    let solids = solids.into();
    let mut planes = Planes::default();
    let mut p = pos.to_f64();
    let orig = vel.to_f64();
    let dt = f64::from(dt);
    let mut v = orig;
    let mut left = dt;
    if !(p.is_finite() && orig.is_finite() && dt.is_finite()) {
        return planes.finish(pos, Vec3::ZERO);
    }
    for _ in 0..MAX_BUMPS {
        if v.length() < MIN_SPEED || left <= 0.0 {
            break;
        }
        let tr = sweep_sphere_d(solids, p, p + v * left, f64::from(radius));
        if tr.start_solid {
            v = Vec3d::ZERO;
            break;
        }
        p = tr.end;
        if !tr.hit {
            break;
        }
        left *= 1.0 - tr.fraction;
        planes.add(tr.normal);
        v = planes.resolve(orig);
    }
    planes.finish(p.to_f32(), v.to_f32())
}

#[derive(Default)]
struct Planes {
    n: [Vec3d; MAX_BUMPS],
    count: usize,
}

impl Planes {
    fn add(&mut self, normal: Vec3d) {
        let known = self.n[..self.count]
            .iter()
            .any(|k| k.dot(normal) > SAME_PLANE);
        if !known && self.count < MAX_BUMPS {
            self.n[self.count] = normal;
            self.count += 1;
        }
    }

    /// Velocity that follows the touched planes: clipped to a single plane if
    /// that is legal against the others, else along a two-plane crease, else
    /// stopped.
    fn resolve(&self, orig: Vec3d) -> Vec3d {
        let planes = &self.n[..self.count];
        for (i, &ni) in planes.iter().enumerate() {
            let clipped = orig - ni * orig.dot(ni);
            let legal = planes
                .iter()
                .enumerate()
                .all(|(j, &nj)| j == i || clipped.dot(nj) >= -CLIP_SLACK);
            if legal {
                return clipped;
            }
        }
        let v = match planes {
            [a, b] => a.cross(*b).normalized().map(|dir| dir * orig.dot(dir)),
            _ => None,
        };
        // A crease velocity pointing against the original intent would make
        // the mover back out of the corner; stopping is steadier.
        v.filter(|v| v.dot(orig) > 0.0).unwrap_or(Vec3d::ZERO)
    }

    fn finish(&self, pos: Vec3, vel: Vec3) -> SlideResult {
        let mut normals = [Vec3::ZERO; MAX_BUMPS];
        for (slot, n) in normals.iter_mut().zip(&self.n[..self.count]) {
            *slot = n.to_f32();
        }
        SlideResult {
            pos,
            vel,
            normals,
            contacts: self.count,
        }
    }
}
