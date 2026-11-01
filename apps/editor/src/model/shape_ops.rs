//! Pure geometry behind the brush tools: slicing, carving, vertex moves and
//! face extrusion. Everything works on a brush's outward plane list and
//! re-validates the result through [`Brush`], so a tool can never hand the
//! document a brush the game would reject.

use mscent_math::{DPlane, Vec3d, epsilon};
use mscent_world::{Brush, TexInfo};

use super::document::{BrushDef, Id};
use super::state::LIMIT;

/// Most vertices the vertex tool handles: its hull search is quartic in the
/// vertex count per drag step.
pub const MAX_VERTS: usize = 48;

/// A brush's planes with one mapping each.
#[derive(Clone, Debug)]
pub struct Parts {
    pub planes: Vec<DPlane>,
    pub infos: Vec<TexInfo>,
}

impl Parts {
    /// Adds `plane`, textured like a default face of `material`.
    pub fn with_plane(&self, plane: DPlane, material: u16) -> Parts {
        let mut out = self.clone();
        out.planes.push(plane);
        out.infos.push(fresh_info(plane, material));
        out
    }
}

/// The default mapping for a new face lying in `plane`.
pub fn fresh_info(plane: DPlane, material: u16) -> TexInfo {
    let mut info = TexInfo::default_for_normal(plane.normal);
    info.material = material;
    info
}

/// The planes and mappings of a brush in its built order.
pub fn parts(def: &BrushDef) -> Option<Parts> {
    let b = def.to_brush(0).ok()?;
    Some(Parts {
        planes: b.planes().to_vec(),
        infos: b.texinfos().to_vec(),
    })
}

/// A brush from `parts` that keeps `like`'s operation and material, or
/// `None` when the planes enclose no volume or leave the editor's range.
pub fn rebuild(id: Id, like: &BrushDef, parts: &Parts) -> Option<BrushDef> {
    let b = Brush::new_textured(&parts.planes, &parts.infos, like.op, 0).ok()?;
    // The planes carry their own materials (a face may be painted
    // differently); only the brush-level default is inherited.
    let mut def = BrushDef::from_brush(id, &b);
    def.material = like.material;
    let (lo, hi) = def.bounds();
    let inside = lo.iter().chain(hi.iter()).all(|v| v.abs() <= LIMIT);
    inside.then_some(def)
}

/// The plane through a view's hidden axis along the line `a`..`b` (view
/// coordinates on world axes `axes`); its normal points to the right of the
/// direction of travel. `None` for a line shorter than `min_len`.
pub fn line_plane(axes: (usize, usize), a: [f64; 2], b: [f64; 2], min_len: f64) -> Option<DPlane> {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let len = dx.hypot(dy);
    if !len.is_finite() || len < min_len {
        return None;
    }
    let mut n = [0.0; 3];
    n[axes.0] = dy / len;
    n[axes.1] = -dx / len;
    let mut p = [0.0; 3];
    p[axes.0] = a[0];
    p[axes.1] = a[1];
    let normal = Vec3d::new(n[0], n[1], n[2]);
    Some(DPlane::new(
        normal,
        normal.dot(Vec3d::new(p[0], p[1], p[2])),
    ))
}

/// Cuts a brush by `cut`: the piece behind it (where the normal points
/// away) and the piece in front. A side the cut leaves empty is `None`.
pub fn slice(
    id_back: Id,
    id_front: Id,
    def: &BrushDef,
    cut: DPlane,
) -> (Option<BrushDef>, Option<BrushDef>) {
    let Some(src) = parts(def) else {
        return (None, None);
    };
    let back = rebuild(id_back, def, &src.with_plane(cut, def.material));
    let front = rebuild(id_front, def, &src.with_plane(cut.flipped(), def.material));
    (back, front)
}

/// What is left of `target` after removing `cutter`, as convex pieces;
/// `None` when the two do not overlap. Pieces keep the target's operation
/// and material, and the faces the cutter exposes take default mappings.
pub fn subtract(target: &BrushDef, cutter: &BrushDef) -> Option<Vec<BrushDef>> {
    let (mut rest, cut) = (parts(target)?, parts(cutter)?);
    let mut pieces = Vec::new();
    for plane in cut.planes {
        let outside = rest.with_plane(plane.flipped(), target.material);
        if let Some(piece) = rebuild(0, target, &outside) {
            pieces.push(piece);
        }
        rest = rest.with_plane(plane, target.material);
    }
    rebuild(0, target, &rest).map(|_| pieces)
}

/// The distinct corners of a brush, or `None` when there are too many for
/// the vertex tool.
pub fn vertices(def: &BrushDef) -> Option<Vec<Vec3d>> {
    let brush = def.to_brush(0).ok()?;
    let mut out: Vec<Vec3d> = Vec::new();
    for v in brush.faces().iter().flat_map(|f| f.vertices()) {
        if !out.iter().any(|o| (*o - *v).length() < 1.0e-6) {
            out.push(*v);
        }
    }
    (out.len() <= MAX_VERTS).then_some(out)
}

/// The convex hull of `points` as outward planes: every plane through three
/// points that has all the others on one side, each listed once.
pub fn hull_planes(points: &[Vec3d]) -> Vec<DPlane> {
    let eps = epsilon::PLANE_EPSILON_F64;
    let mut out: Vec<DPlane> = Vec::new();
    for (i, &a) in points.iter().enumerate() {
        for (j, &b) in points.iter().enumerate().skip(i + 1) {
            for &c in points.iter().skip(j + 1) {
                let Some(n) = (b - a).cross(c - a).normalized() else {
                    continue;
                };
                let d = n.dot(a);
                let (mut above, mut below) = (false, false);
                for p in points {
                    let s = n.dot(*p) - d;
                    above |= s > eps;
                    below |= s < -eps;
                }
                let plane = match (above, below) {
                    (false, _) => DPlane::new(n, d),
                    (true, false) => DPlane::new(-n, -d),
                    (true, true) => continue,
                };
                let same = |q: &DPlane| {
                    (q.normal - plane.normal).length() < 1.0e-7 && (q.dist - plane.dist).abs() < eps
                };
                if !out.iter().any(same) {
                    out.push(plane);
                }
            }
        }
    }
    out
}

/// `old` reshaped to the hull of `points`. Each new face inherits the
/// mapping of the old face it most resembles; a face unlike any gets the
/// default. `None` unless the result is a valid brush with as many corners
/// as before (a corner dragged inside, or onto another, is refused).
pub fn reshape(old: &BrushDef, points: &[Vec3d]) -> Option<BrushDef> {
    let src = parts(old)?;
    let planes = hull_planes(points);
    let infos = planes
        .iter()
        .map(|p| {
            let best = src
                .planes
                .iter()
                .zip(&src.infos)
                .map(|(q, t)| (q.normal.dot(p.normal), t))
                .max_by(|x, y| x.0.total_cmp(&y.0));
            match best {
                Some((dot, t)) if dot > 0.7 => *t,
                _ => fresh_info(*p, old.material),
            }
        })
        .collect();
    let def = rebuild(old.id, old, &Parts { planes, infos })?;
    (vertices(&def)?.len() == points.len()).then_some(def)
}

/// The prism that pushes face `plane` of `def` out by `dist` (positive).
/// Its base is the face itself and its sides stand on the face's edges.
pub fn prism(id: Id, def: &BrushDef, plane: usize, dist: f64) -> Option<BrushDef> {
    let brush = def.to_brush(0).ok()?;
    let n = brush.planes().get(plane)?.normal;
    let v = brush.faces().get(plane)?.vertices();
    let base = *v.first()?;
    let mut planes = vec![
        DPlane::new(n, n.dot(base) + dist),
        DPlane::new(-n, -n.dot(base)),
    ];
    for (i, &a) in v.iter().enumerate() {
        let b = v[(i + 1) % v.len()];
        let side = (b - a).cross(n).normalized()?;
        planes.push(DPlane::new(side, side.dot(a)));
    }
    let mut infos: Vec<TexInfo> = planes
        .iter()
        .map(|p| fresh_info(*p, def.material))
        .collect();
    // The pushed face keeps the texture the face had.
    infos[0] = *brush.texinfos().get(plane)?;
    rebuild(id, def, &Parts { planes, infos })
}

/// `def` with the planes in `which` moved along their normals by `dist`
/// (negative pulls a face into the brush).
pub fn move_planes(def: &BrushDef, which: &[usize], dist: f64) -> Option<BrushDef> {
    let mut src = parts(def)?;
    for &i in which {
        src.planes.get_mut(i)?.dist += dist;
    }
    rebuild(def.id, def, &src)
}
