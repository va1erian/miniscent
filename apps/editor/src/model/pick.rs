//! Picking a brush face with a ray, for the 3D view's face tool.
//!
//! A face is the plane of a brush. Which side of it can be seen follows the
//! CSG rules: an add brush shows its outward side, a subtract brush (a room
//! carved from the solid world) shows its inward side. A hit counts only
//! where that really is the surface of the compiled level, so faces buried in
//! solid or covered by a later brush are skipped, and the ray must arrive from
//! the visible side.

use mscent_math::{Polygon, Vec3d};
use mscent_render::View;
use mscent_world::{Brush, Op};

use super::document::{Document, Id};

/// How far from the face the "is it empty here" probes are taken.
const PROBE: f64 = 1.0e-3;
/// Slack for "the hit is inside the face".
const INSIDE: f64 = 1.0e-6;

/// A half line.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ray {
    pub origin: Vec3d,
    /// Need not be unit length, but must not be zero.
    pub dir: Vec3d,
}

/// One face of one brush: the brush's plane number, in the order of
/// `BrushDef::to_brush`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FaceRef {
    pub brush: Id,
    pub plane: usize,
}

/// A ray hit.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FaceHit {
    pub face: FaceRef,
    /// Distance along the ray, in units of `dir`.
    pub t: f64,
    pub point: Vec3d,
}

/// The ray through the point `(nx, ny)` of a picture, both in `0..=1` from
/// the top left, for a camera looking along `view` with vertical field of
/// view `fov_y` and a picture of `aspect` (width over height). `None` for
/// junk input.
pub fn view_ray(view: &View, fov_y: f32, nx: f32, ny: f32, aspect: f32) -> Option<Ray> {
    let ok = [fov_y, nx, ny, aspect].iter().all(|v| v.is_finite()) && aspect > 0.0;
    if !ok || !view.pos.is_finite() {
        return None;
    }
    let half = (fov_y * 0.5).tan();
    let cam = mscent_math::Vec3::new(
        (nx * 2.0 - 1.0) * half * aspect,
        (1.0 - ny * 2.0) * half,
        -1.0,
    );
    let dir = view.orientation.rotate(cam).normalized()?;
    Some(Ray {
        origin: view.pos.to_f64(),
        dir: dir.to_f64(),
    })
}

/// The side of plane `plane` of `brush` that shows: the outward normal of an
/// add brush, the inward one of a subtract brush.
pub fn visible_normal(brush: &Brush, plane: usize) -> Option<Vec3d> {
    let n = brush.planes().get(plane)?.normal;
    Some(if brush.op() == Op::Add { n } else { -n })
}

/// Whether the compiled level is empty at `p`: the last brush that contains
/// the point decides, and the world starts solid.
fn empty_at(brushes: &[Option<Brush>], p: Vec3d) -> bool {
    brushes
        .iter()
        .rev()
        .flatten()
        .find(|b| b.contains(p))
        .is_some_and(|b| b.op() == Op::Subtract)
}

/// Builds every brush in list order, so the CSG order is the list position.
pub fn build_all(doc: &Document) -> Vec<Option<Brush>> {
    doc.brushes()
        .iter()
        .enumerate()
        .map(|(i, b)| b.to_brush(i as u32).ok())
        .collect()
}

/// The nearest visible face along `ray`, if any.
pub fn pick_face(doc: &Document, ray: &Ray) -> Option<FaceHit> {
    if !ray.origin.is_finite() || !ray.dir.is_finite() || ray.dir.length() < 1.0e-12 {
        return None;
    }
    let brushes = build_all(doc);
    let mut best: Option<FaceHit> = None;
    for (def, brush) in doc.brushes().iter().zip(&brushes) {
        let Some(brush) = brush else { continue };
        for (plane, pl) in brush.planes().iter().enumerate() {
            let Some(nv) = visible_normal(brush, plane) else {
                continue;
            };
            // The ray must come from the visible side.
            if ray.dir.dot(nv) >= -1.0e-9 {
                continue;
            }
            let t = -pl.signed_distance(ray.origin) / ray.dir.dot(pl.normal);
            if !t.is_finite() || t <= 1.0e-9 || best.is_some_and(|b| b.t <= t) {
                continue;
            }
            let point = ray.origin + ray.dir * t;
            let inside = brush
                .planes()
                .iter()
                .enumerate()
                .all(|(k, p)| k == plane || p.signed_distance(point) <= INSIDE);
            let surface =
                empty_at(&brushes, point + nv * PROBE) && !empty_at(&brushes, point - nv * PROBE);
            if inside && surface {
                best = Some(FaceHit {
                    face: FaceRef {
                        brush: def.id,
                        plane,
                    },
                    t,
                    point,
                });
            }
        }
    }
    best
}

/// The polygon of a face as the brush builds it, and its visible normal.
pub fn face_polygon(doc: &Document, face: FaceRef) -> Option<(Polygon, Vec3d)> {
    let index = doc.brush_index(face.brush)?;
    let brush = doc.brushes().get(index)?.to_brush(index as u32).ok()?;
    let normal = visible_normal(&brush, face.plane)?;
    Some((brush.faces().get(face.plane)?.clone(), normal))
}
