//! Moving solids: convex brushes under a rigid pose (M10.3).
//!
//! A mover's cells are built once, in the coordinates the brushes were
//! authored in, and a sweep is carried out in that local frame. A sphere's
//! radius does not change under a rigid transform, so the exact cell sweep
//! works unchanged, rounded edges and vertices included.

use crate::cell::{Cell, CellPlane};
use crate::prim::Contact;
use crate::tuning::{MIN_SPEED, PRUNE_MARGIN, SKIN};
use mscent_math::{Quat, Vec3d};
use mscent_world::Brush;
use std::sync::Arc;

/// Where a mover is, relative to how it was authored: a point `x` goes to
/// `rot (x - pivot) + pivot + offset`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pose {
    pub rot: Quat,
    pub pivot: Vec3d,
    pub offset: Vec3d,
}

impl Pose {
    /// As authored.
    pub const IDENTITY: Self = Self {
        rot: Quat::IDENTITY,
        pivot: Vec3d::ZERO,
        offset: Vec3d::ZERO,
    };

    pub fn translated(offset: Vec3d) -> Self {
        Self {
            offset,
            ..Self::IDENTITY
        }
    }

    /// Turned `radians` about the line through `pivot` along `axis`.
    pub fn turned(axis: mscent_math::Vec3, radians: f32, pivot: Vec3d) -> Self {
        Self {
            rot: Quat::from_axis_angle(axis, radians),
            pivot,
            offset: Vec3d::ZERO,
        }
    }

    pub fn is_finite(&self) -> bool {
        let q = self.rot;
        [q.x, q.y, q.z, q.w].iter().all(|c| c.is_finite())
            && self.pivot.is_finite()
            && self.offset.is_finite()
    }

    /// The pose a fraction `s` of the way from `a` to `b`. Translation is
    /// linear and rotation is about `b`'s pivot, so a hinge swings on its
    /// hinge rather than cutting across the arc.
    pub fn between(a: &Pose, b: &Pose, s: f32) -> Pose {
        if s <= 0.0 {
            return *a;
        }
        if s >= 1.0 {
            return *b;
        }
        // `a` rewritten about `b`'s pivot: the same motion, a new offset.
        let turned = Frame::rotation(a.rot).apply(b.pivot - a.pivot);
        let from_offset = turned + a.pivot - b.pivot + a.offset;
        Pose {
            rot: a.rot.slerp(b.rot, s),
            pivot: b.pivot,
            offset: from_offset.lerp(b.offset, f64::from(s)),
        }
    }

    /// The angle in radians between this pose's rotation and `o`'s.
    pub fn angle_to(&self, o: &Pose) -> f32 {
        let dot = self.rot.dot(o.rot).abs().min(1.0);
        2.0 * dot.acos()
    }
}

/// A rigid transform in `f64`, so a far-from-origin mover keeps its precision.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Frame {
    rows: [Vec3d; 3],
    t: Vec3d,
}

impl Frame {
    fn rotation(q: Quat) -> Frame {
        let (x, y, z, w) = (
            f64::from(q.x),
            f64::from(q.y),
            f64::from(q.z),
            f64::from(q.w),
        );
        // Normalized here, in `f64`, so the rows are orthonormal to rounding.
        let n = (x * x + y * y + z * z + w * w)
            .sqrt()
            .max(f64::MIN_POSITIVE);
        let (x, y, z, w) = (x / n, y / n, z / n, w / n);
        let rows = [
            Vec3d::new(
                1.0 - 2.0 * (y * y + z * z),
                2.0 * (x * y - w * z),
                2.0 * (x * z + w * y),
            ),
            Vec3d::new(
                2.0 * (x * y + w * z),
                1.0 - 2.0 * (x * x + z * z),
                2.0 * (y * z - w * x),
            ),
            Vec3d::new(
                2.0 * (x * z - w * y),
                2.0 * (y * z + w * x),
                1.0 - 2.0 * (x * x + y * y),
            ),
        ];
        Frame {
            rows,
            t: Vec3d::ZERO,
        }
    }

    pub(crate) fn of(pose: &Pose) -> Frame {
        let mut frame = Frame::rotation(pose.rot);
        frame.t = pose.pivot + pose.offset - frame.apply(pose.pivot);
        frame
    }

    /// Rotation only: for directions.
    pub(crate) fn apply(&self, v: Vec3d) -> Vec3d {
        let [a, b, c] = self.rows;
        Vec3d::new(a.dot(v), b.dot(v), c.dot(v))
    }

    fn apply_inverse(&self, v: Vec3d) -> Vec3d {
        let [a, b, c] = self.rows;
        a * v.x + b * v.y + c * v.z
    }

    pub(crate) fn to_world(self, p: Vec3d) -> Vec3d {
        self.apply(p) + self.t
    }

    pub(crate) fn to_local(self, p: Vec3d) -> Vec3d {
        self.apply_inverse(p - self.t)
    }
}

/// The geometry of a mover, in authored coordinates. Shared between a level's
/// arena and every restart of it.
#[derive(Clone, Debug, PartialEq)]
pub struct MoverShape {
    cells: Vec<Cell>,
    lo: Vec3d,
    hi: Vec3d,
}

impl MoverShape {
    pub fn from_brushes<'a>(brushes: impl Iterator<Item = &'a Brush>) -> Self {
        let mut shape = Self {
            cells: Vec::new(),
            lo: Vec3d::new(f64::MAX, f64::MAX, f64::MAX),
            hi: Vec3d::new(f64::MIN, f64::MIN, f64::MIN),
        };
        for brush in brushes {
            let bounds = brush.bounds();
            shape.lo = shape.lo.min(bounds.min.to_f64());
            shape.hi = shape.hi.max(bounds.max.to_f64());
            let planes: Vec<CellPlane> = brush
                .planes()
                .iter()
                .map(|p| CellPlane {
                    normal: p.normal,
                    dist: p.dist,
                })
                .collect();
            shape.cells.push(Cell::build(&planes));
        }
        shape
    }

    fn corners(&self) -> [Vec3d; 8] {
        let (a, b) = (self.lo, self.hi);
        [
            Vec3d::new(a.x, a.y, a.z),
            Vec3d::new(b.x, a.y, a.z),
            Vec3d::new(a.x, b.y, a.z),
            Vec3d::new(b.x, b.y, a.z),
            Vec3d::new(a.x, a.y, b.z),
            Vec3d::new(b.x, a.y, b.z),
            Vec3d::new(a.x, b.y, b.z),
            Vec3d::new(b.x, b.y, b.z),
        ]
    }

    fn sweep_local(&self, from: Vec3d, d: Vec3d, radius: f64, limit: f64) -> Option<Contact> {
        let mut best: Option<Contact> = None;
        for cell in &self.cells {
            // A centre already inside moves freely, so a bug or a bad spawn
            // cannot freeze a body in place for the rest of the level.
            if cell.contains_point(from) {
                continue;
            }
            let reach = best.map_or(limit, |b| b.t);
            let found = cell.sweep(from, d, radius, reach);
            if let Some(c) = found.filter(|c| best.is_none_or(|b| c.t < b.t)) {
                best = Some(c);
            }
        }
        best
    }

    /// Whether a centre is inside any brush, where sweeps ignore that brush.
    pub(crate) fn contains_centre(&self, p: Vec3d) -> bool {
        self.cells.iter().any(|c| c.contains_point(p))
    }

    /// The deepest overlap of a sphere with any brush: the depth, and the unit
    /// direction that leads out of it. `None` when it overlaps nothing.
    pub(crate) fn overlap(&self, p: Vec3d, radius: f64) -> Option<(f64, Vec3d)> {
        let mut deepest: Option<(f64, Vec3d)> = None;
        for cell in &self.cells {
            let out = match cell.nearest_outside(p) {
                Some(q) => (radius - (p - q).length(), (p - q).normalized()),
                // A centre inside has no way out to offer; only a stop.
                None if cell.contains_point(p) => (radius, None),
                None => continue,
            };
            if out.0 > 0.0 && deepest.is_none_or(|(d, _)| out.0 > d) {
                deepest = Some((out.0, out.1.unwrap_or(Vec3d::ZERO)));
            }
        }
        deepest
    }

    /// Moves a sphere along `d` in the local frame, sliding along what it
    /// meets. Returns where it ended and whether it touched anything.
    pub(crate) fn slide(&self, mut p: Vec3d, mut d: Vec3d, radius: f64) -> (Vec3d, bool) {
        let mut touched = false;
        for _ in 0..crate::slide::MAX_BUMPS {
            let Some(c) = self.sweep_local(p, d, radius, 1.0) else {
                return (p + d, touched);
            };
            touched = true;
            let rest = d * (1.0 - c.t);
            p = p + d * c.t + c.normal * SKIN;
            d = rest - c.normal * rest.dot(c.normal);
            if d.length() < MIN_SPEED {
                break;
            }
        }
        (p, touched)
    }
}

/// A mover at its current pose.
#[derive(Clone, Debug, PartialEq)]
pub struct MoverSolid {
    pub(crate) shape: Arc<MoverShape>,
    pub(crate) frame: Frame,
    pub(crate) lo: Vec3d,
    pub(crate) hi: Vec3d,
}

impl MoverSolid {
    pub fn new(shape: Arc<MoverShape>) -> Self {
        let mut solid = Self {
            shape,
            frame: Frame::of(&Pose::IDENTITY),
            lo: Vec3d::ZERO,
            hi: Vec3d::ZERO,
        };
        solid.set_frame(Frame::of(&Pose::IDENTITY));
        solid
    }

    /// The middle of its bounds where it is now.
    pub fn centre(&self) -> Vec3d {
        self.lo.lerp(self.hi, 0.5)
    }

    pub fn shape(&self) -> &MoverShape {
        &self.shape
    }

    pub fn set_pose(&mut self, pose: &Pose) {
        self.set_frame(Frame::of(pose));
    }

    pub(crate) fn set_frame(&mut self, frame: Frame) {
        self.frame = frame;
        let (lo, hi) = frame_bounds(&self.shape, frame);
        self.lo = lo;
        self.hi = hi;
    }

    /// Whether anything on the segment within `radius` could reach this mover.
    pub(crate) fn near_segment(&self, from: Vec3d, to: Vec3d, radius: f64) -> bool {
        let m = radius + PRUNE_MARGIN;
        let (a, b) = (from.min(to), from.max(to));
        a.x - m <= self.hi.x
            && b.x + m >= self.lo.x
            && a.y - m <= self.hi.y
            && b.y + m >= self.lo.y
            && a.z - m <= self.hi.z
            && b.z + m >= self.lo.z
    }

    /// First contact, in world coordinates.
    pub(crate) fn sweep(&self, from: Vec3d, d: Vec3d, radius: f64, limit: f64) -> Option<Contact> {
        let local_from = self.frame.to_local(from);
        let local_d = self.frame.apply_inverse(d);
        let c = self.shape.sweep_local(local_from, local_d, radius, limit)?;
        Some(Contact {
            t: c.t,
            normal: self.frame.apply(c.normal),
        })
    }
}

pub(crate) fn frame_bounds(shape: &MoverShape, frame: Frame) -> (Vec3d, Vec3d) {
    let first = frame.to_world(shape.lo);
    let mut lo = first;
    let mut hi = first;
    for c in shape.corners() {
        let w = frame.to_world(c);
        lo = lo.min(w);
        hi = hi.max(w);
    }
    (lo, hi)
}
