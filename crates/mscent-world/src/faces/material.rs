//! Picks the texture mapping (and material) of a surface fragment from the
//! brush plane that made it.

use crate::brush::{Brush, Op};
use crate::texinfo::TexInfo;
use mscent_math::{DPlane, Polygon, Vec3d, epsilon};

pub(super) fn centroid(poly: &Polygon) -> Vec3d {
    let v = poly.vertices();
    let sum = v.iter().fold(Vec3d::ZERO, |acc, &p| acc + p);
    sum * (1.0 / v.len() as f64)
}

/// The last brush (by order) with a face on `plane` that covers `at` decides,
/// and its plane's [`TexInfo`] is used. `plane` faces the empty side: a
/// subtract brush's outward plane faces the solid, so it appears flipped; an
/// add brush's outward plane faces the room. With no owner the default
/// mapping for the plane is used.
pub(super) fn texinfo_for(sorted: &[&Brush], plane: &DPlane, at: Vec3d) -> TexInfo {
    sorted
        .iter()
        .rev()
        .find_map(|b| owned_texinfo(b, plane, at))
        .unwrap_or_else(|| TexInfo::default_for_normal(plane.normal))
}

fn owned_texinfo(brush: &Brush, plane: &DPlane, at: Vec3d) -> Option<TexInfo> {
    let want = match brush.op() {
        Op::Add => *plane,
        Op::Subtract => plane.flipped(),
    };
    let inside = brush
        .planes()
        .iter()
        .all(|p| p.signed_distance(at) <= epsilon::PLANE_EPSILON_F64);
    let index = brush
        .planes()
        .iter()
        .position(|p| super::same_plane(p, &want))?;
    inside
        .then(|| brush.texinfos().get(index).copied())
        .flatten()
}
