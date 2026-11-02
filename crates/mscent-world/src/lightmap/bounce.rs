//! Cheap one-bounce pass: every face becomes a single patch that re-emits the
//! average direct light it received, and each luxel gathers from the patches
//! it can see. One shadow ray per luxel and patch keeps rooms that are lit
//! only through a door from going black, at a cost of luxels x faces rays.
//! Voxel cells gather from the same patches and from patches of their own
//! (see `cells_bounce.rs`).

use super::bake::{Scene, luminance, rows_to_grids};
use super::parallel::par_map;
use super::uv::FaceProjection;
use super::{LightFace, Luxel};
use mscent_math::Vec3;
use std::f32::consts::PI;

/// A face, or the surface of a voxel chunk, treated as one diffuse emitter.
pub(super) struct Patch {
    /// Where its rays end: on the surface of a face, or in the air in front of
    /// a chunk's surface.
    pub centre: Vec3,
    pub normal: Vec3,
    pub area: f32,
    /// Light the patch sends back out: average direct colour times albedo.
    pub radiosity: Vec3,
    /// Shines in every direction, as a chunk's crumpled surface does, instead
    /// of only out of its front.
    pub omni: bool,
}

/// Adds the bounce contribution of `patches` to the direct-light `grids` (one
/// per face). Does nothing when albedo or gain is zero.
pub(super) fn add_bounce(
    s: &Scene,
    rows: &[(usize, u32)],
    patches: &[Patch],
    grids: &mut [Vec<Luxel>],
) {
    if s.cfg.albedo <= 0.0 || s.cfg.bounce_gain <= 0.0 {
        return;
    }
    if patches.iter().all(|p| luminance(p.radiosity) <= 1e-6) {
        return;
    }
    let extra = par_map(s.cfg.workers, rows.len(), |i| {
        if s.cancelled() {
            return Vec::new();
        }
        gather_row(s, patches, rows[i].0, rows[i].1)
    });
    let extra = rows_to_grids(s.proj, extra);
    for (grid, add) in grids.iter_mut().zip(extra) {
        for (l, a) in grid.iter_mut().zip(add) {
            l.color = l.color + a.color;
            l.dir = l.dir + a.dir;
        }
    }
}

/// One patch per face, from the direct light of its luxels.
pub(super) fn face_patches(s: &Scene, grids: &[Vec<Luxel>]) -> Vec<Patch> {
    let faces = s.faces.iter().zip(s.proj).zip(grids);
    faces
        .map(|((face, proj), grid)| {
            let n = grid.len().max(1) as f32;
            let avg = grid.iter().fold(Vec3::ZERO, |a, l| a + l.color) * (1.0 / n);
            Patch {
                centre: centroid(face),
                normal: proj.normal,
                area: polygon_area(face, proj),
                radiosity: avg * s.cfg.albedo,
                omni: false,
            }
        })
        .collect()
}

fn centroid(face: &LightFace) -> Vec3 {
    let sum = face.vertices.iter().fold(Vec3::ZERO, |a, &v| a + v);
    sum * (1.0 / face.vertices.len().max(1) as f32)
}

/// Area of the fan triangulation projected on the face normal.
fn polygon_area(face: &LightFace, proj: &FaceProjection) -> f32 {
    let Some(&v0) = face.vertices.first() else {
        return 0.0;
    };
    let twice: f32 = face
        .vertices
        .windows(2)
        .skip(1)
        .map(|w| (w[0] - v0).cross(w[1] - v0).dot(proj.normal))
        .sum();
    twice.abs() * 0.5
}

fn gather_row(s: &Scene, patches: &[Patch], face: usize, row: u32) -> Vec<Luxel> {
    let p = &s.proj[face];
    (0..p.width)
        .map(|x| {
            let point = p.luxel_sample(&s.faces[face].vertices, x, row);
            let mut acc = Luxel {
                color: Vec3::ZERO,
                dir: Vec3::ZERO,
            };
            for patch in patches
                .iter()
                .enumerate()
                .filter(|&(j, _)| j != face)
                .map(|(_, p)| p)
            {
                if let Some((c, to_patch)) = gather_from(s, p, point, patch) {
                    acc.color = acc.color + c;
                    acc.dir = acc.dir + p.to_tangent(to_patch) * luminance(c);
                }
            }
            acc
        })
        .collect()
}

/// Light reaching `point` from one patch and the unit direction towards it.
fn gather_from(s: &Scene, p: &FaceProjection, point: Vec3, patch: &Patch) -> Option<(Vec3, Vec3)> {
    let (colour, dir) = transfer(s, patch, point, p.normal)?;
    let bias = s.cfg.normal_bias;
    let from = (point + p.normal * bias).to_f64();
    let target = (patch.centre + patch.normal * bias).to_f64();
    s.occ.segment_clear(from, target).then_some((colour, dir))
}

/// What the patch sends to a surface point facing `normal`, before shadows
/// (disc form factor, so nearby patches do not blow up), and the unit
/// direction towards the patch.
pub(super) fn transfer(
    s: &Scene,
    patch: &Patch,
    point: Vec3,
    normal: Vec3,
) -> Option<(Vec3, Vec3)> {
    if luminance(patch.radiosity) <= 1e-6 {
        return None;
    }
    let to = patch.centre - point;
    let dist2 = to.length_squared();
    let dir = to.normalized()?;
    let cos_here = dir.dot(normal);
    let cos_there = if patch.omni {
        0.5
    } else {
        -dir.dot(patch.normal)
    };
    if cos_here <= 0.0 || cos_there <= 0.0 {
        return None;
    }
    let form = patch.area * cos_here * cos_there / (PI * dist2 + patch.area);
    Some((patch.radiosity * (form * s.cfg.bounce_gain), dir))
}
