//! The shapes a piece can have. A hard shape is a convex brush, and a soft one a
//! [`Prim`]. Both are stored as what the file says, so a file writes back the same.

use mscent_math::{DPlane, Vec3d, epsilon};

use crate::brush::{Brush, BrushError, Op};
use crate::voxel::Prim;

/// Most sides of an n-gon prism.
pub const MAX_NGON_SIDES: u8 = 32;
/// Most corners of a plate.
pub const MAX_PLATE_POINTS: usize = 8;
/// Fewest sides or corners of either, below which nothing has any volume.
pub(super) const MIN_SIDES: usize = 3;

/// A convex solid. Its corners snap to the grid brushes use.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Hard {
    /// A box around `centre`, `half` wide each way.
    Cube { centre: Vec3d, half: Vec3d },
    /// A prism of `sides` sides from `a` to `b`, tapering from radius `ra` to `rb`.
    Ngon {
        a: Vec3d,
        b: Vec3d,
        ra: f64,
        rb: f64,
        sides: u8,
    },
    /// A flat polygon, `thick` through. Only the first `count` points count.
    Plate {
        thick: f64,
        count: u8,
        points: [Vec3d; MAX_PLATE_POINTS],
    },
}

/// A round shape for voxels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Soft {
    Sphere {
        centre: Vec3d,
        radius: f64,
    },
    RoundCone {
        a: Vec3d,
        b: Vec3d,
        ra: f64,
        rb: f64,
    },
    Cuboid {
        min: Vec3d,
        max: Vec3d,
    },
    Spheroid {
        centre: Vec3d,
        axis: Vec3d,
        along: f64,
        across: f64,
    },
    HexPrism {
        centre: Vec3d,
        axis: Vec3d,
        side: Vec3d,
        apothem: f64,
        half_depth: f64,
    },
}

impl Soft {
    /// The shape voxels are edited with, or `None` for one out of range.
    pub fn prim(&self) -> Option<Prim> {
        match *self {
            Self::Sphere { centre, radius } => Prim::sphere(centre, radius),
            Self::RoundCone { a, b, ra, rb } => Prim::round_cone(a, b, ra, rb),
            Self::Cuboid { min, max } => Prim::cuboid(min, max),
            Self::Spheroid {
                centre,
                axis,
                along,
                across,
            } => Prim::spheroid(centre, axis, along, across),
            Self::HexPrism {
                centre,
                axis,
                side,
                apothem,
                half_depth,
            } => Prim::hex_prism(centre, axis, side, apothem, half_depth),
        }
    }
}

fn v(x: f64, y: f64, z: f64) -> Vec3d {
    Vec3d::new(x, y, z)
}

/// `a` turned by `angle` about the unit vector `axis`.
fn turned(a: Vec3d, axis: Vec3d, angle: f64) -> Vec3d {
    let (s, c) = angle.sin_cos();
    a * c + axis.cross(a) * s + axis * (axis.dot(a) * (1.0 - c))
}

/// Outward-wound plane triples of a tapering prism, or `None` if `a` is `b`.
fn ngon_triples(a: Vec3d, b: Vec3d, ra: f64, rb: f64, sides: u8) -> Option<Vec<[Vec3d; 3]>> {
    let d = (b - a).normalized()?;
    let helper = if d.z.abs() < 0.9 {
        v(0.0, 0.0, 1.0)
    } else {
        v(1.0, 0.0, 0.0)
    };
    let first = d.cross(helper).normalized()?;
    let step = core::f64::consts::TAU / f64::from(sides);
    let spoke = |i: u8| turned(first, d, step * f64::from(i));
    let mut out: Vec<[Vec3d; 3]> = (0..sides)
        .map(|i| {
            [
                a + spoke(i) * ra,
                a + spoke(i + 1) * ra,
                b + spoke(i + 1) * rb,
            ]
        })
        .collect();
    out.push([b + spoke(0) * rb, b + spoke(1) * rb, b + spoke(2) * rb]);
    out.push([a + spoke(2) * ra, a + spoke(1) * ra, a + spoke(0) * ra]);
    Some(out)
}

/// Outward-wound plane triples of a plate. Its normal comes from its first three
/// points, so either winding of the corners gives the same solid. `None` if they
/// lie on a line.
fn plate_triples(thick: f64, points: &[Vec3d]) -> Option<Vec<[Vec3d; 3]>> {
    let &[p0, p1, p2, ..] = points else {
        return None;
    };
    let n = (p1 - p0).cross(p2 - p0).normalized()? * (thick * 0.5);
    let mut out = vec![[p0 + n, p1 + n, p2 + n], [p2 - n, p1 - n, p0 - n]];
    for (i, &a) in points.iter().enumerate() {
        let b = points[(i + 1) % points.len()];
        out.push([a - n, b - n, b + n]);
    }
    Some(out)
}

impl Hard {
    /// The solid as a brush. An error means the shape has no volume once
    /// snapped: a shape too small for the grid, or points in a line.
    pub fn brush(&self, op: Op, order: u32) -> Result<Brush, BrushError> {
        let triples = match *self {
            Self::Cube { centre, half } => {
                return Brush::cuboid(centre - half, centre + half, op, order);
            }
            Self::Ngon {
                a,
                b,
                ra,
                rb,
                sides,
            } => ngon_triples(a, b, ra, rb, sides),
            Self::Plate {
                thick,
                count,
                ref points,
            } => plate_triples(thick, &points[..usize::from(count)]),
        }
        .ok_or(BrushError::Degenerate)?;
        let planes = triples
            .iter()
            .map(|t| DPlane::from_snapped_points(t[0], t[1], t[2], epsilon::SNAP_GRID))
            .collect::<Option<Vec<_>>>()
            .ok_or(BrushError::Degenerate)?;
        Brush::new(&planes, op, order)
    }
}
