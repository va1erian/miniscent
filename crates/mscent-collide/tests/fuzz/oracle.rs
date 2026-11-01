//! Independent oracle for the fuzzer: evaluates the ORDERED brush list
//! directly, never touching the BSP, so a bug in the compiler or in the
//! collision code cannot hide itself.
//!
//! The generated levels are unions and differences of axis-aligned boxes, so
//! the boundary of the solid lies on box faces. The solid point nearest a
//! sphere centre is then always the clamped projection onto one box face, or
//! onto the line or point where two or three faces meet. Those candidates are
//! enumerated exactly, so rounded edges and corners are judged precisely
//! (point sampling of the sphere surface would miss a shallow edge bite).

use mscent_math::Vec3d;
use mscent_world::{Brush, Op};

/// A sphere may sit this much inside the ideal surface before it counts as
/// penetrating. The sweep backs off its SKIN (1e-3) along the contact normal;
/// when the sphere is wedged between two surfaces whose gap equals its
/// diameter, that back-off necessarily presses it into the other one, and a
/// few such presses can stack within one chain of moves. Real tunnelling or
/// tracking errors are orders of magnitude larger.
const TOLERANCE: f64 = 5.0e-3;
/// Step off a candidate boundary point to test which side is solid.
const PROBE: f64 = 1.0e-7;
const SLACK: f64 = 1.0e-9;

/// True if `p` is solid: the world starts as rock, and each brush in order
/// either carves it out or fills it back in.
pub fn solid(brushes: &[Brush], p: Vec3d) -> bool {
    let mut inside = true;
    for b in brushes {
        if b.contains(p) {
            inside = b.op() == Op::Add;
        }
    }
    inside
}

fn get(v: Vec3d, k: usize) -> f64 {
    [v.x, v.y, v.z][k]
}

/// One box face as a degenerate box: `lo == hi` on its own axis.
#[derive(Clone, Copy)]
struct Face {
    lo: [f64; 3],
    hi: [f64; 3],
}

pub struct Oracle {
    faces: Vec<Face>,
}

impl Oracle {
    pub fn new(brushes: &[Brush]) -> Self {
        let mut faces = Vec::new();
        for b in brushes {
            let bb = b.bounds();
            let (lo, hi) = (bb.min.to_f64(), bb.max.to_f64());
            let lo = [lo.x, lo.y, lo.z];
            let hi = [hi.x, hi.y, hi.z];
            for axis in 0..3 {
                for val in [lo[axis], hi[axis]] {
                    let (mut l, mut h) = (lo, hi);
                    l[axis] = val;
                    h[axis] = val;
                    faces.push(Face { lo: l, hi: h });
                }
            }
        }
        Self { faces }
    }

    /// The point of the intersection of `faces` nearest to `c`, if they meet.
    fn nearest(faces: &[&Face], c: Vec3d) -> Option<Vec3d> {
        let mut q = [0.0; 3];
        for (k, slot) in q.iter_mut().enumerate() {
            let lo = faces.iter().map(|f| f.lo[k]).fold(f64::MIN, f64::max);
            let hi = faces.iter().map(|f| f.hi[k]).fold(f64::MAX, f64::min);
            if lo > hi + SLACK {
                return None;
            }
            *slot = get(c, k).clamp(lo, hi.max(lo));
        }
        Some(Vec3d::new(q[0], q[1], q[2]))
    }

    /// True if the solid has a point within `r - TOLERANCE` of `c`.
    pub fn overlaps(&self, brushes: &[Brush], c: Vec3d, r: f64) -> bool {
        if solid(brushes, c) {
            return true;
        }
        let reach = r + 1.0;
        let near: Vec<&Face> = self
            .faces
            .iter()
            .filter(|f| {
                (0..3).all(|k| get(c, k) >= f.lo[k] - reach && get(c, k) <= f.hi[k] + reach)
            })
            .collect();
        let limit = r - TOLERANCE;
        let touches = |set: &[&Face]| {
            Self::nearest(set, c)
                .is_some_and(|q| (q - c).length() < limit && Self::solid_beside(brushes, q))
        };
        for (i, a) in near.iter().enumerate() {
            if touches(&[a]) {
                return true;
            }
            for (j, b) in near.iter().enumerate().skip(i + 1) {
                if touches(&[a, b]) {
                    return true;
                }
                if near.iter().skip(j + 1).any(|d| touches(&[a, b, d])) {
                    return true;
                }
            }
        }
        false
    }

    /// True if solid lies against boundary point `q` on any side.
    fn solid_beside(brushes: &[Brush], q: Vec3d) -> bool {
        (0..8).any(|m| {
            let s = |bit: i32| if m & bit == 0 { PROBE } else { -PROBE };
            solid(brushes, q + Vec3d::new(s(1), s(2), s(4)))
        })
    }

    /// First point along `a -> b` (sampled every `step`) where the sphere
    /// overlaps solid, as a fraction of the path.
    pub fn first_overlap(
        &self,
        brushes: &[Brush],
        a: Vec3d,
        b: Vec3d,
        r: f64,
        step: f64,
    ) -> Option<f64> {
        let len = (b - a).length();
        let n = (len / step).ceil().max(1.0) as u32;
        (0..=n)
            .map(|i| f64::from(i) / f64::from(n))
            .find(|&t| self.overlaps(brushes, a.lerp(b, t), r))
    }
}
