//! Picks the material of a surface fragment from the brush that made it.

use crate::brush::{Brush, DEFAULT_MATERIAL, Op};
use mscent_math::{DPlane, Polygon, Vec3d, epsilon};

pub(super) fn centroid(poly: &Polygon) -> Vec3d {
    let v = poly.vertices();
    let sum = v.iter().fold(Vec3d::ZERO, |acc, &p| acc + p);
    sum * (1.0 / v.len() as f64)
}

/// The last brush (by order) with a face on `plane` that covers `at` decides.
/// `plane` faces the empty side: a subtract brush's outward plane faces the
/// solid, so it appears flipped; an add brush's outward plane faces the room.
pub(super) fn material_for(sorted: &[&Brush], plane: &DPlane, at: Vec3d) -> u16 {
    sorted
        .iter()
        .rev()
        .find(|b| owns_surface(b, plane, at))
        .map_or(DEFAULT_MATERIAL, |b| b.material())
}

fn owns_surface(brush: &Brush, plane: &DPlane, at: Vec3d) -> bool {
    let want = match brush.op() {
        Op::Add => *plane,
        Op::Subtract => plane.flipped(),
    };
    brush.planes().iter().any(|p| super::same_plane(p, &want))
        && brush
            .planes()
            .iter()
            .all(|p| p.signed_distance(at) <= epsilon::PLANE_EPSILON_F64)
}
