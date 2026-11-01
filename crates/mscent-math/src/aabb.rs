//! Axis-aligned bounding box in `f32`.

use crate::plane::Plane;
use crate::vec::Vec3;

/// Box with inclusive bounds. An `empty()` box contains nothing and is the
/// identity for [`Aabb::union`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Aabb {
    pub min: Vec3,
    pub max: Vec3,
}

impl Aabb {
    pub const fn new(min: Vec3, max: Vec3) -> Self {
        Self { min, max }
    }

    pub const fn empty() -> Self {
        Self::new(
            Vec3::new(f32::INFINITY, f32::INFINITY, f32::INFINITY),
            Vec3::new(f32::NEG_INFINITY, f32::NEG_INFINITY, f32::NEG_INFINITY),
        )
    }

    pub fn from_points(pts: &[Vec3]) -> Self {
        pts.iter().fold(Self::empty(), |b, &p| b.including(p))
    }

    pub fn is_empty(&self) -> bool {
        self.min.x > self.max.x || self.min.y > self.max.y || self.min.z > self.max.z
    }

    pub fn including(self, p: Vec3) -> Self {
        Self::new(self.min.min(p), self.max.max(p))
    }

    pub fn union(self, o: Self) -> Self {
        Self::new(self.min.min(o.min), self.max.max(o.max))
    }

    /// Grows the box by `r` on every side (e.g. by a sphere radius).
    pub fn expanded(self, r: f32) -> Self {
        let d = Vec3::new(r, r, r);
        Self::new(self.min - d, self.max + d)
    }

    pub fn center(&self) -> Vec3 {
        (self.min + self.max) * 0.5
    }

    pub fn half_extents(&self) -> Vec3 {
        (self.max - self.min) * 0.5
    }

    pub fn contains_point(&self, p: Vec3) -> bool {
        p.x >= self.min.x
            && p.x <= self.max.x
            && p.y >= self.min.y
            && p.y <= self.max.y
            && p.z >= self.min.z
            && p.z <= self.max.z
    }

    pub fn intersects(&self, o: &Self) -> bool {
        self.min.x <= o.max.x
            && self.max.x >= o.min.x
            && self.min.y <= o.max.y
            && self.max.y >= o.min.y
            && self.min.z <= o.max.z
            && self.max.z >= o.min.z
    }

    /// Which side of `plane` the whole box is on, or `On` if it straddles
    /// the plane (the box is cut by it). Uses the projected radius of the
    /// box onto the plane normal, so it is exact for any plane orientation.
    pub fn classify_plane(&self, plane: &Plane) -> BoxSide {
        let e = self.half_extents();
        let n = plane.normal;
        let radius = e.x * n.x.abs() + e.y * n.y.abs() + e.z * n.z.abs();
        let d = plane.signed_distance(self.center());
        if d > radius {
            BoxSide::Front
        } else if d < -radius {
            BoxSide::Back
        } else {
            BoxSide::Straddling
        }
    }
}

/// Result of classifying a box against a plane.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BoxSide {
    Front,
    Back,
    Straddling,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unit() -> Aabb {
        Aabb::new(Vec3::new(0.0, 0.0, 0.0), Vec3::new(1.0, 1.0, 1.0))
    }

    #[test]
    fn empty_is_union_identity() {
        assert!(Aabb::empty().is_empty());
        assert_eq!(Aabb::empty().union(unit()), unit());
        assert!(!unit().is_empty());
    }

    #[test]
    fn from_points_bounds_them() {
        let b = Aabb::from_points(&[Vec3::new(1.0, -2.0, 3.0), Vec3::new(-1.0, 2.0, 0.0)]);
        assert_eq!(b.min, Vec3::new(-1.0, -2.0, 0.0));
        assert_eq!(b.max, Vec3::new(1.0, 2.0, 3.0));
        assert!(Aabb::from_points(&[]).is_empty());
    }

    #[test]
    fn containment_and_overlap() {
        let b = unit();
        assert!(b.contains_point(Vec3::new(1.0, 0.5, 0.0)));
        assert!(!b.contains_point(Vec3::new(1.01, 0.5, 0.0)));
        let touching = Aabb::new(Vec3::new(1.0, 0.0, 0.0), Vec3::new(2.0, 1.0, 1.0));
        let apart = Aabb::new(Vec3::new(1.5, 0.0, 0.0), Vec3::new(2.0, 1.0, 1.0));
        assert!(b.intersects(&touching));
        assert!(!b.intersects(&apart));
        assert!(b.expanded(0.5).intersects(&apart));
    }

    #[test]
    fn plane_classification() {
        let x_plane = |d| Plane::new(Vec3::new(1.0, 0.0, 0.0), d);
        assert_eq!(unit().classify_plane(&x_plane(-1.0)), BoxSide::Front);
        assert_eq!(unit().classify_plane(&x_plane(2.0)), BoxSide::Back);
        assert_eq!(unit().classify_plane(&x_plane(0.5)), BoxSide::Straddling);
        // Diagonal plane through two opposite corners straddles the cube.
        let diag = Plane::from_point_normal(Vec3::new(0.5, 0.5, 0.5), Vec3::new(0.6, 0.0, 0.8));
        assert_eq!(unit().classify_plane(&diag), BoxSide::Straddling);
    }
}
