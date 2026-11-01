//! T-junction repair: every vertex that lies inside another face's edge is
//! inserted into that edge, so neighbours share identical vertices and the
//! rasterizer cannot open cracks between them.

use super::Face;
use mscent_math::{Polygon, Vec3d, epsilon};

pub(super) fn fix(faces: &mut [Face]) {
    let mut points: Vec<Vec3d> = faces
        .iter()
        .flat_map(|f| f.polygon.vertices().iter().copied())
        .collect();
    // Sorted by x so each edge only scans the vertices in its x range.
    points.sort_by(|a, b| a.x.total_cmp(&b.x));
    for face in faces {
        let verts = face.polygon.vertices();
        let n = verts.len();
        let mut out = Vec::with_capacity(n + 4);
        for i in 0..n {
            out.push(verts[i]);
            out.extend(interior_points(&points, verts[i], verts[(i + 1) % n]));
        }
        if let Some(p) = Polygon::new(out) {
            face.polygon = p;
        }
    }
}

/// Points strictly inside the edge `a`-`b`, ordered from `a` to `b`.
fn interior_points(points: &[Vec3d], a: Vec3d, b: Vec3d) -> Vec<Vec3d> {
    let eps = epsilon::PLANE_EPSILON_F64;
    let lo = a.x.min(b.x) - eps;
    let hi = a.x.max(b.x) + eps;
    let start = points.partition_point(|p| p.x < lo);
    let mut hits: Vec<(f64, Vec3d)> = points[start..]
        .iter()
        .take_while(|p| p.x <= hi)
        .filter_map(|&p| edge_param(a, b, p).map(|t| (t, p)))
        .collect();
    hits.sort_by(|x, y| x.0.total_cmp(&y.0));
    let mut kept: Vec<Vec3d> = Vec::new();
    for (_, p) in hits {
        if kept.last().is_none_or(|&q| (p - q).length() > eps) {
            kept.push(p);
        }
    }
    kept
}

/// Parameter of `p` along `a`-`b` when it lies on the open segment.
fn edge_param(a: Vec3d, b: Vec3d, p: Vec3d) -> Option<f64> {
    let eps = epsilon::PLANE_EPSILON_F64;
    let d = b - a;
    let len = d.length();
    if len <= eps {
        return None;
    }
    let along = (p - a).dot(d) / len;
    if along <= eps || along >= len - eps {
        return None;
    }
    let closest = a + d * (along / len);
    ((p - closest).length() <= eps).then_some(along / len)
}
