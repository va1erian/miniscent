//! Joins coplanar neighbours into larger convex polygons.
//!
//! Two faces join when they share a whole edge (traversed in opposite
//! directions), have the same plane and TexInfo, and the union stays
//! convex. Partial overlaps stay separate and are handled as T-junctions.
//! The pass is quadratic per plane group, which is fine for the group sizes
//! brush levels produce.

use super::{Face, same_plane};
use mscent_math::{Polygon, Vec3d, epsilon};

pub(super) fn merge_coplanar(faces: Vec<Face>) -> Vec<Face> {
    let mut groups: Vec<Vec<Face>> = Vec::new();
    for face in faces {
        let slot = groups
            .iter()
            .position(|g| g[0].texinfo == face.texinfo && same_plane(&g[0].plane, &face.plane));
        match slot {
            Some(i) => groups[i].push(face),
            None => groups.push(vec![face]),
        }
    }
    groups.into_iter().flat_map(merge_group).collect()
}

fn merge_group(mut group: Vec<Face>) -> Vec<Face> {
    while let Some((i, j, poly)) = find_join(&group) {
        let other = group.remove(j);
        let keep = &mut group[i];
        keep.polygon = poly;
        keep.front_leaves.extend(other.front_leaves);
        keep.back_leaves.extend(other.back_leaves);
        for list in [&mut keep.front_leaves, &mut keep.back_leaves] {
            list.sort();
            list.dedup();
        }
    }
    group
}

fn find_join(group: &[Face]) -> Option<(usize, usize, Polygon)> {
    for (i, a) in group.iter().enumerate() {
        for (j, b) in group.iter().enumerate().skip(i + 1) {
            if let Some(p) = try_join(a, b) {
                return Some((i, j, p));
            }
        }
    }
    None
}

fn same_point(a: Vec3d, b: Vec3d) -> bool {
    (a - b).length() <= epsilon::PLANE_EPSILON_F64
}

fn try_join(a: &Face, b: &Face) -> Option<Polygon> {
    let (va, vb) = (a.polygon.vertices(), b.polygon.vertices());
    let (na, nb) = (va.len(), vb.len());
    for i in 0..na {
        let (p, q) = (va[i], va[(i + 1) % na]);
        for j in 0..nb {
            if same_point(vb[j], q) && same_point(vb[(j + 1) % nb], p) {
                return union_polygon(va, i, vb, j, a.plane.normal);
            }
        }
    }
    None
}

/// Walks `a` from the end of the shared edge around to its start, then `b`
/// past the shared edge back to where `a` began.
fn union_polygon(va: &[Vec3d], i: usize, vb: &[Vec3d], j: usize, n: Vec3d) -> Option<Polygon> {
    let (na, nb) = (va.len(), vb.len());
    let mut verts: Vec<Vec3d> = (0..na).map(|k| va[(i + 1 + k) % na]).collect();
    verts.extend((2..nb).map(|k| vb[(j + k) % nb]));
    drop_collinear(&mut verts);
    if verts.len() < 3 || !is_convex(&verts, n) {
        return None;
    }
    Polygon::new(verts)
}

/// Removes vertices lying on the line through their neighbours.
fn drop_collinear(verts: &mut Vec<Vec3d>) {
    let mut i = 0;
    while verts.len() > 3 && i < verts.len() {
        let len = verts.len();
        let (prev, cur, next) = (verts[(i + len - 1) % len], verts[i], verts[(i + 1) % len]);
        let off_line = (cur - prev).cross(next - cur).length();
        if off_line <= epsilon::PLANE_EPSILON_F64 * (next - prev).length() {
            verts.remove(i);
            i = i.saturating_sub(1);
        } else {
            i += 1;
        }
    }
}

fn is_convex(verts: &[Vec3d], n: Vec3d) -> bool {
    let len = verts.len();
    (0..len).all(|i| {
        let (a, b, c) = (verts[i], verts[(i + 1) % len], verts[(i + 2) % len]);
        (b - a).cross(c - b).dot(n) >= 0.0
    })
}
