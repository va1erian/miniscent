//! Planes in `f32` (runtime) and `f64` (CSG build), with the classification
//! rules every other crate relies on.
//!
//! A plane is the set of points `p` with `normal . p == dist`. The front side
//! is where `normal . p - dist` is positive. Points within the epsilon band
//! around the plane are `On`; the band is half-width, so total thickness is
//! twice the epsilon.

use crate::epsilon;
use crate::vec::{Vec3, Vec3d};

/// Where a single point lies relative to a plane.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Front,
    Back,
    On,
}

/// Where a set of points (a polygon's vertices) lies relative to a plane.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PolySide {
    /// At least one vertex in front, none behind.
    Front,
    /// At least one vertex behind, none in front.
    Back,
    /// Every vertex on the plane.
    On,
    /// Vertices on both sides; the plane cuts the polygon.
    Spanning,
}

impl PolySide {
    /// Folds per-vertex sides into one polygon verdict.
    pub fn from_sides(sides: impl IntoIterator<Item = Side>) -> Self {
        let (mut front, mut back) = (false, false);
        for s in sides {
            match s {
                Side::Front => front = true,
                Side::Back => back = true,
                Side::On => {}
            }
        }
        match (front, back) {
            (true, true) => Self::Spanning,
            (true, false) => Self::Front,
            (false, true) => Self::Back,
            (false, false) => Self::On,
        }
    }
}

/// Classifies a signed distance with a half-width band `eps`. NaN is treated
/// as `On` so garbage never silently picks a side.
fn side_of<T: PartialOrd + Copy + core::ops::Neg<Output = T>>(d: T, eps: T) -> Side {
    if d > eps {
        Side::Front
    } else if d < -eps {
        Side::Back
    } else {
        Side::On
    }
}

/// `f32` plane for runtime queries (collision, rendering).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Plane {
    pub normal: Vec3,
    pub dist: f32,
}

impl Plane {
    pub const fn new(normal: Vec3, dist: f32) -> Self {
        Self { normal, dist }
    }

    /// Plane through `point` with unit-length `normal`.
    pub fn from_point_normal(point: Vec3, normal: Vec3) -> Self {
        Self::new(normal, normal.dot(point))
    }

    pub fn signed_distance(&self, p: Vec3) -> f32 {
        self.normal.dot(p) - self.dist
    }

    pub fn classify_point(&self, p: Vec3) -> Side {
        side_of(self.signed_distance(p), epsilon::PLANE_EPSILON_F32)
    }

    pub fn classify_points(&self, pts: &[Vec3]) -> PolySide {
        PolySide::from_sides(pts.iter().map(|&p| self.classify_point(p)))
    }

    /// The same plane facing the other way.
    pub fn flipped(&self) -> Self {
        Self::new(-self.normal, -self.dist)
    }
}

/// `f64` plane used while building CSG.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DPlane {
    pub normal: Vec3d,
    pub dist: f64,
}

impl DPlane {
    pub const fn new(normal: Vec3d, dist: f64) -> Self {
        Self { normal, dist }
    }

    /// Plane through three points, wound counter-clockwise when seen from the
    /// front. `None` if the points are collinear, coincident or not finite.
    pub fn from_points(a: Vec3d, b: Vec3d, c: Vec3d) -> Option<Self> {
        let normal = (b - a).cross(c - a).normalized()?;
        Some(Self::new(normal, normal.dot(a)))
    }

    /// Like [`DPlane::from_points`] after snapping the points to `grid`. This
    /// is how brush planes get their exact, reproducible representation.
    pub fn from_snapped_points(a: Vec3d, b: Vec3d, c: Vec3d, grid: f64) -> Option<Self> {
        Self::from_points(a.snapped(grid), b.snapped(grid), c.snapped(grid))
    }

    pub fn signed_distance(&self, p: Vec3d) -> f64 {
        self.normal.dot(p) - self.dist
    }

    pub fn classify_point(&self, p: Vec3d) -> Side {
        side_of(self.signed_distance(p), epsilon::PLANE_EPSILON_F64)
    }

    pub fn classify_points(&self, pts: &[Vec3d]) -> PolySide {
        PolySide::from_sides(pts.iter().map(|&p| self.classify_point(p)))
    }

    pub fn flipped(&self) -> Self {
        Self::new(-self.normal, -self.dist)
    }

    /// Narrows to the runtime plane once the build is finished.
    pub fn to_f32(&self) -> Plane {
        Plane::new(self.normal.to_f32(), self.dist as f32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const EPS64: f64 = epsilon::PLANE_EPSILON_F64;
    const EPS32: f32 = epsilon::PLANE_EPSILON_F32;

    fn up64() -> DPlane {
        DPlane::new(Vec3d::new(0.0, 0.0, 1.0), 2.0)
    }

    #[test]
    fn f64_classification_around_epsilon() {
        let p = up64();
        let at = |dz: f64| p.classify_point(Vec3d::new(5.0, -3.0, 2.0 + dz));
        assert_eq!(at(0.0), Side::On);
        assert_eq!(at(EPS64 * 0.5), Side::On);
        assert_eq!(at(-EPS64 * 0.5), Side::On);
        // Just inside and just outside the band edge (the exact edge is not
        // representable after adding 2.0, so probe either side of it).
        assert_eq!(at(EPS64 * 0.999), Side::On);
        assert_eq!(at(-EPS64 * 0.999), Side::On);
        assert_eq!(at(EPS64 * 1.001), Side::Front);
        assert_eq!(at(-EPS64 * 1.001), Side::Back);
        assert_eq!(at(EPS64 * 2.0), Side::Front);
        assert_eq!(at(-EPS64 * 2.0), Side::Back);
        assert_eq!(at(1.0), Side::Front);
        assert_eq!(at(-1.0), Side::Back);
    }

    #[test]
    fn f32_classification_around_epsilon() {
        let p = Plane::new(Vec3::new(1.0, 0.0, 0.0), 1.0);
        let at = |dx: f32| p.classify_point(Vec3::new(1.0 + dx, 9.0, 9.0));
        assert_eq!(at(0.0), Side::On);
        assert_eq!(at(EPS32 * 0.5), Side::On);
        assert_eq!(at(-EPS32 * 0.5), Side::On);
        assert_eq!(at(EPS32 * 4.0), Side::Front);
        assert_eq!(at(-EPS32 * 4.0), Side::Back);
    }

    #[test]
    fn nan_is_on() {
        assert_eq!(
            up64().classify_point(Vec3d::new(f64::NAN, 0.0, 0.0)),
            Side::On
        );
    }

    #[test]
    fn polygon_verdicts() {
        let p = up64();
        let v = |z: f64| Vec3d::new(0.0, 0.0, z);
        assert_eq!(p.classify_points(&[v(2.0), v(2.0)]), PolySide::On);
        assert_eq!(p.classify_points(&[v(2.0), v(3.0)]), PolySide::Front);
        assert_eq!(p.classify_points(&[v(2.0), v(1.0)]), PolySide::Back);
        assert_eq!(p.classify_points(&[v(3.0), v(1.0)]), PolySide::Spanning);
        // A vertex inside the band does not make a polygon span.
        assert_eq!(
            p.classify_points(&[v(2.0 + EPS64 * 0.5), v(3.0)]),
            PolySide::Front
        );
        assert_eq!(p.classify_points(&[]), PolySide::On);
    }

    #[test]
    fn from_points_orientation_and_degenerate() {
        let (a, b, c) = (
            Vec3d::new(0.0, 0.0, 1.0),
            Vec3d::new(1.0, 0.0, 1.0),
            Vec3d::new(0.0, 1.0, 1.0),
        );
        let p = DPlane::from_points(a, b, c).unwrap();
        assert_eq!(p.normal, Vec3d::new(0.0, 0.0, 1.0));
        assert_eq!(p.dist, 1.0);
        assert!(DPlane::from_points(a, b, b).is_none());
        assert!(DPlane::from_points(a, a, a).is_none());
        assert!(DPlane::from_points(a, b, Vec3d::new(f64::NAN, 0.0, 0.0)).is_none());
    }

    #[test]
    fn snapped_points_give_reproducible_plane() {
        let g = epsilon::SNAP_GRID;
        let jitter = 0.01;
        let p1 = DPlane::from_snapped_points(
            Vec3d::new(0.0 + jitter, 0.0, 1.0),
            Vec3d::new(1.0, jitter, 1.0),
            Vec3d::new(0.0, 1.0, 1.0 - jitter),
            g,
        )
        .unwrap();
        assert_eq!(p1.normal, Vec3d::new(0.0, 0.0, 1.0));
        assert_eq!(p1.dist, 1.0);
    }

    #[test]
    fn flip_swaps_sides() {
        let p = up64();
        let q = Vec3d::new(0.0, 0.0, 5.0);
        assert_eq!(p.classify_point(q), Side::Front);
        assert_eq!(p.flipped().classify_point(q), Side::Back);
        assert_eq!(p.to_f32().flipped().classify_point(q.to_f32()), Side::Back);
    }
}
