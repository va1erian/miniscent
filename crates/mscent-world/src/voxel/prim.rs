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
    /// An ellipsoid of revolution: `along` the unit `axis`, `across` elsewhere.
    Spheroid {
        centre: Vec3d,
        axis: Vec3d,
        along: f64,
        across: f64,
    },
    /// A hexagonal prism, the cell of a comb. `side` is the unit direction
    /// perpendicular to `axis` that points at a corner.
    HexPrism {
        centre: Vec3d,
        axis: Vec3d,
        side: Vec3d,
        apothem: f64,
        half_depth: f64,
    },
}

/// Half the width of a hexagon across its corners, per unit of apothem.
const HEX_CORNER: f64 = 1.154_700_538_379_251_7;
/// Cosine of 30 degrees: the slanted flats' normal along a corner direction.
const HEX_SLANT: f64 = 0.866_025_403_784_438_6;

/// The box of half-width `r` around `centre`.
fn cube(centre: Vec3d, r: f64) -> (Vec3d, Vec3d) {
    let r = Vec3d::new(r, r, r);
    (centre - r, centre + r)
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

    /// The radius at its thinnest: half its least width.
    pub fn thinnest(&self) -> f64 {
        match self.0 {
            Shape::Sphere { radius, .. } => radius,
            Shape::RoundCone {
                radius_a, radius_b, ..
            } => radius_a.min(radius_b),
            Shape::Cuboid { min, max } => {
                let d = (max - min) * 0.5;
                d.x.min(d.y).min(d.z)
            }
            Shape::Spheroid { along, across, .. } => along.min(across),
            Shape::HexPrism {
                apothem,
                half_depth,
                ..
            } => apothem.min(half_depth),
        }
    }

    /// The same shape `by` larger on every side, or `None` past the legal range.
    pub fn grown(&self, by: f64) -> Option<Self> {
        match self.0 {
            Shape::Sphere { centre, radius } => Self::sphere(centre, radius + by),
            Shape::RoundCone {
                a,
                b,
                radius_a,
                radius_b,
            } => Self::round_cone(a, b, radius_a + by, radius_b + by),
            Shape::Cuboid { min, max } => {
                let d = Vec3d::new(by, by, by);
                Self::cuboid(min - d, max + d)
            }
            Shape::Spheroid {
                centre,
                axis,
                along,
                across,
            } => Self::spheroid(centre, axis, along + by, across + by),
            Shape::HexPrism {
                centre,
                axis,
                side,
                apothem,
                half_depth,
            } => Self::hex_prism(centre, axis, side, apothem + by, half_depth + by),
        }
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

    /// An ellipsoid with a unit `axis` (any length but zero is accepted and
    /// normalised) and two radii: along the axis, and across it.
    pub fn spheroid(centre: Vec3d, axis: Vec3d, along: f64, across: f64) -> Option<Self> {
        let axis = axis.normalized()?;
        let ok = in_range(centre) && positive(along) && positive(across);
        ok.then_some(Self(Shape::Spheroid {
            centre,
            axis,
            along,
            across,
        }))
    }

    /// A hexagonal prism: `apothem` is half the distance between opposite
    /// flats, `half_depth` half its length along `axis`, and `corner_hint`
    /// picks which way a corner points (any direction not along the axis).
    pub fn hex_prism(
        centre: Vec3d,
        axis: Vec3d,
        corner_hint: Vec3d,
        apothem: f64,
        half_depth: f64,
    ) -> Option<Self> {
        let axis = axis.normalized()?;
        let side = (corner_hint - axis * corner_hint.dot(axis)).normalized()?;
        let ok = in_range(centre) && positive(apothem) && positive(half_depth);
        ok.then_some(Self(Shape::HexPrism {
            centre,
            axis,
            side,
            apothem,
            half_depth,
        }))
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
            Shape::Spheroid {
                centre,
                axis,
                along,
                across,
            } => {
                let d = p - centre;
                let h = d.dot(axis);
                let q = (d - axis * h).length();
                // Scaled back by the smaller radius: wrong in size, right in sign.
                let k = ((h / along).powi(2) + (q / across).powi(2)).sqrt();
                (k - 1.0) * along.min(across)
            }
            Shape::HexPrism {
                centre,
                axis,
                side,
                apothem,
                half_depth,
            } => {
                let d = p - centre;
                let (h, x) = (d.dot(axis), d.dot(side).abs());
                let y = d.dot(axis.cross(side)).abs();
                // Corners lie along `side`; two flats face the cross direction
                // and four slant 30 degrees round from the corner direction.
                let flat = (x * HEX_SLANT + y * 0.5).max(y);
                (flat - apothem).max(h.abs() - half_depth)
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
            Shape::Spheroid {
                centre,
                along,
                across,
                ..
            } => cube(centre, along.max(across)),
            Shape::HexPrism {
                centre,
                apothem,
                half_depth,
                ..
            } => cube(centre, apothem * HEX_CORNER + half_depth),
        }
    }
}
