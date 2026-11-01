//! Sutherland-Hodgman clipping of convex polygons to the view frustum.
//!
//! Clipping happens in camera space, before the perspective divide, so a
//! polygon that crosses the near plane never produces a vertex at or behind
//! the eye. Texture coordinates are affine in 3D, so they interpolate
//! linearly along clipped edges.

use mscent_math::{Vec2, Vec3};

use crate::camera::Frustum;

/// Upper bound on input polygon size; a convex polygon gains at most one
/// vertex per clip plane.
pub const MAX_POLYGON_VERTICES: usize = 256;

/// A camera-space vertex with texture coordinates (in texture repeats).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ClipVertex {
    pub pos: Vec3,
    pub uv: Vec2,
}

impl ClipVertex {
    fn lerp(self, o: Self, t: f32) -> Self {
        Self {
            pos: self.pos.lerp(o.pos, t),
            uv: Vec2::new(
                self.uv.x + (o.uv.x - self.uv.x) * t,
                self.uv.y + (o.uv.y - self.uv.y) * t,
            ),
        }
    }
}

/// Clip one polygon against one plane, appending the result to `out`.
fn clip_plane(frustum: &Frustum, plane: usize, input: &[ClipVertex], out: &mut Vec<ClipVertex>) {
    out.clear();
    let Some(&last) = input.last() else { return };
    let mut prev = last;
    let mut prev_d = frustum.distance(plane, prev.pos);
    for &cur in input {
        let cur_d = frustum.distance(plane, cur.pos);
        if (cur_d >= 0.0) != (prev_d >= 0.0) {
            // The signs differ, so the denominator is non-zero.
            out.push(prev.lerp(cur, prev_d / (prev_d - cur_d)));
        }
        if cur_d >= 0.0 {
            out.push(cur);
        }
        prev = cur;
        prev_d = cur_d;
    }
}

/// Clip `input` to all six frustum planes. Returns the clipped polygon in
/// the same winding, or an empty vector when it is fully outside, has fewer
/// than three vertices, is too large, or contains a non-finite coordinate.
pub fn clip_polygon(frustum: &Frustum, input: &[ClipVertex]) -> Vec<ClipVertex> {
    let finite = |v: &ClipVertex| v.pos.is_finite() && v.uv.x.is_finite() && v.uv.y.is_finite();
    if input.len() < 3 || input.len() > MAX_POLYGON_VERTICES || !input.iter().all(finite) {
        return Vec::new();
    }
    let mut a = input.to_vec();
    let mut b = Vec::with_capacity(input.len() + 6);
    for plane in 0..6 {
        clip_plane(frustum, plane, &a, &mut b);
        core::mem::swap(&mut a, &mut b);
        if a.len() < 3 {
            return Vec::new();
        }
    }
    a
}

#[cfg(test)]
#[path = "clip_tests.rs"]
mod clip_tests;
