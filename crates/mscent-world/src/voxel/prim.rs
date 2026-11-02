//! Solid shapes that dabs and generators are made of.

use mscent_math::Vec3d;

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

/// A validated shape: finite, with positive sizes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Prim(Shape);

fn positive(v: f64) -> bool {
    v.is_finite() && v > 0.0
}

impl Prim {
    pub fn sphere(centre: Vec3d, radius: f64) -> Option<Self> {
        (centre.is_finite() && positive(radius)).then_some(Self(Shape::Sphere { centre, radius }))
    }

    /// A tapered capsule from `a` to `b`. With equal radii it is a capsule.
    pub fn round_cone(a: Vec3d, b: Vec3d, radius_a: f64, radius_b: f64) -> Option<Self> {
        let ok = a.is_finite() && b.is_finite() && positive(radius_a) && positive(radius_b);
        ok.then_some(Self(Shape::RoundCone {
            a,
            b,
            radius_a,
            radius_b,
        }))
    }

    pub fn cuboid(min: Vec3d, max: Vec3d) -> Option<Self> {
        let ok = min.is_finite()
            && max.is_finite()
            && positive(max.x - min.x)
            && positive(max.y - min.y)
            && positive(max.z - min.z);
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
