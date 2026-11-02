//! Solid shapes that dabs and generators are made of.

use mscent_math::Vec3d;

use crate::polytope::MAX_COORD;

#[derive(Clone, Copy, Debug, PartialEq)]
enum Shape {
    Sphere {
        centre: Vec3d,
        radius: f64,
    },
    RoundCone {
        a: Vec3d,
        b: Vec3d,
        radius_a: f64,
        radius_b: f64,
    },
    Cuboid {
        min: Vec3d,
        max: Vec3d,
    },
}

/// A validated shape: in the range brushes use, with positive sizes. The range
/// matters because a larger shape's distances overflow to infinity and then NaN.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Prim(Shape);

fn positive(v: f64) -> bool {
    v > 0.0 && v <= MAX_COORD
}

fn in_range(p: Vec3d) -> bool {
    [p.x, p.y, p.z].iter().all(|v| v.abs() <= MAX_COORD)
}

impl Prim {
    pub fn sphere(centre: Vec3d, radius: f64) -> Option<Self> {
        (in_range(centre) && positive(radius)).then_some(Self(Shape::Sphere { centre, radius }))
    }

    /// A tapered capsule from `a` to `b`. With equal radii it is a capsule.
    pub fn round_cone(a: Vec3d, b: Vec3d, radius_a: f64, radius_b: f64) -> Option<Self> {
        let ok = in_range(a) && in_range(b) && positive(radius_a) && positive(radius_b);
        ok.then_some(Self(Shape::RoundCone {
            a,
            b,
            radius_a,
            radius_b,
        }))
    }

    pub fn cuboid(min: Vec3d, max: Vec3d) -> Option<Self> {
        let ok = in_range(min) && in_range(max) && max.x > min.x && max.y > min.y && max.z > min.z;
        ok.then_some(Self(Shape::Cuboid { min, max }))
    }

    /// Signed distance from `p`, negative inside. Exact except for the round
    /// cone, where it is close enough for a surface: its sign is right.
    pub fn distance(&self, p: Vec3d) -> f64 {
        match self.0 {
            Shape::Sphere { centre, radius } => (p - centre).length() - radius,
            Shape::RoundCone {
                a,
                b,
                radius_a,
                radius_b,
            } => {
                let ab = b - a;
                let len2 = ab.dot(ab);
                let t = if len2 > 0.0 {
                    ((p - a).dot(ab) / len2).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                (p - (a + ab * t)).length() - (radius_a + (radius_b - radius_a) * t)
            }
            Shape::Cuboid { min, max } => {
                let q = [
                    (min.x - p.x).max(p.x - max.x),
                    (min.y - p.y).max(p.y - max.y),
                    (min.z - p.z).max(p.z - max.z),
                ];
                let out = Vec3d::new(q[0].max(0.0), q[1].max(0.0), q[2].max(0.0));
                out.length() + q[0].max(q[1]).max(q[2]).min(0.0)
            }
        }
    }

    /// Corners of a box around the shape.
    pub fn bounds(&self) -> (Vec3d, Vec3d) {
        match self.0 {
            Shape::Sphere { centre, radius } => {
                let r = Vec3d::new(radius, radius, radius);
                (centre - r, centre + r)
            }
            Shape::RoundCone {
                a,
                b,
                radius_a,
                radius_b,
            } => {
                let r = radius_a.max(radius_b);
                (
                    Vec3d::new(a.x.min(b.x) - r, a.y.min(b.y) - r, a.z.min(b.z) - r),
                    Vec3d::new(a.x.max(b.x) + r, a.y.max(b.y) + r, a.z.max(b.z) + r),
                )
            }
            Shape::Cuboid { min, max } => (min, max),
        }
    }
}
