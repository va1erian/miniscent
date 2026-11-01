//! Planar UV projection: maps a face onto a grid of luxels.

use super::{LightFace, LightmapError};
use mscent_math::Vec3;

/// Largest luxel count along one side of a face. Bigger faces get coarser
/// luxels instead of failing, which keeps one huge floor from eating the atlas.
pub const MAX_FACE_LUXELS: u32 = 128;

/// Tangent frame and luxel grid of one face.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FaceProjection {
    /// A point on the face plane; the frame's origin.
    pub origin: Vec3,
    pub tangent: Vec3,
    pub bitangent: Vec3,
    /// Unit surface normal; `tangent x bitangent == normal`.
    pub normal: Vec3,
    /// Lower bounds of the face along tangent and bitangent, from `origin`.
    pub u_min: f32,
    pub v_min: f32,
    /// World size of one luxel after any coarsening.
    pub luxel_size: f32,
    pub width: u32,
    pub height: u32,
}

impl FaceProjection {
    /// World position of the centre of luxel `(x, y)`.
    pub fn luxel_center(&self, x: u32, y: u32) -> Vec3 {
        let u = self.u_min + (x as f32 + 0.5) * self.luxel_size;
        let v = self.v_min + (y as f32 + 0.5) * self.luxel_size;
        self.origin + self.tangent * u + self.bitangent * v
    }

    /// Where to light luxel `(x, y)`: its centre, or, when the centre falls
    /// outside the (convex) face polygon `verts`, the nearest point of the
    /// polygon nudged inwards. A grid that is not a whole number of luxels
    /// across hangs over the face edge, and a centre past the edge sits inside
    /// the neighbouring wall, which would shadow the whole edge row black.
    pub fn luxel_sample(&self, verts: &[Vec3], x: u32, y: u32) -> Vec3 {
        let flat = |i: usize| {
            let d = verts[i % verts.len()] - self.origin;
            [d.dot(self.tangent), d.dot(self.bitangent)]
        };
        let q = [
            self.u_min + (x as f32 + 0.5) * self.luxel_size,
            self.v_min + (y as f32 + 0.5) * self.luxel_size,
        ];
        match nearest_inside(verts.len(), flat, q, self.luxel_size * 0.02) {
            Some([u, v]) => self.origin + self.tangent * u + self.bitangent * v,
            None => self.luxel_center(x, y),
        }
    }

    /// Luxel containing the world point's projection, clamped to the grid.
    pub fn nearest_luxel(&self, p: Vec3) -> (u32, u32) {
        let d = p - self.origin;
        let pick = |c: f32, min: f32, n: u32| {
            let i = ((c - min) / self.luxel_size).floor();
            // NaN falls to zero through the cast; the clamp bounds the rest.
            (i.max(0.0) as u32).min(n.saturating_sub(1))
        };
        (
            pick(d.dot(self.tangent), self.u_min, self.width),
            pick(d.dot(self.bitangent), self.v_min, self.height),
        )
    }

    /// Expresses a world-space vector in the tangent frame.
    pub fn to_tangent(&self, w: Vec3) -> Vec3 {
        Vec3::new(
            w.dot(self.tangent),
            w.dot(self.bitangent),
            w.dot(self.normal),
        )
    }
}

/// `None` if `q` is inside the convex polygon of `n` corners given by `pt`
/// (or the polygon cannot hold a point), else the closest boundary point
/// moved `inset` towards the polygon's centroid.
fn nearest_inside(
    n: usize,
    pt: impl Fn(usize) -> [f32; 2],
    q: [f32; 2],
    inset: f32,
) -> Option<[f32; 2]> {
    if n < 3 {
        return None;
    }
    let (mut all_pos, mut all_neg) = (true, true);
    let mut best: Option<(f32, [f32; 2])> = None;
    let mut sum = [0.0f32; 2];
    for i in 0..n {
        let (a, b) = (pt(i), pt(i + 1));
        let ab = [b[0] - a[0], b[1] - a[1]];
        let side = ab[0] * (q[1] - a[1]) - ab[1] * (q[0] - a[0]);
        all_pos &= side >= 0.0;
        all_neg &= side <= 0.0;
        let len2 = ab[0] * ab[0] + ab[1] * ab[1];
        let t = if len2 > 0.0 {
            (((q[0] - a[0]) * ab[0] + (q[1] - a[1]) * ab[1]) / len2).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let p = [a[0] + ab[0] * t, a[1] + ab[1] * t];
        let d = (p[0] - q[0]).powi(2) + (p[1] - q[1]).powi(2);
        if best.is_none_or(|(bd, _)| d < bd) {
            best = Some((d, p));
        }
        sum = [sum[0] + a[0], sum[1] + a[1]];
    }
    if all_pos || all_neg {
        return None;
    }
    let (_, p) = best?;
    let c = [sum[0] / n as f32, sum[1] / n as f32];
    let (dx, dy) = (c[0] - p[0], c[1] - p[1]);
    let dist = dx.hypot(dy);
    let step = inset.min(dist);
    let moved = if dist > 0.0 {
        [p[0] + dx / dist * step, p[1] + dy / dist * step]
    } else {
        p
    };
    moved.iter().all(|v| v.is_finite()).then_some(moved)
}

/// Picks a tangent perpendicular to `n`, built from the world axis least
/// aligned with it so the cross product is well conditioned.
fn tangent_frame(n: Vec3) -> Option<(Vec3, Vec3)> {
    let (ax, ay, az) = (n.x.abs(), n.y.abs(), n.z.abs());
    let axis = if ax <= ay && ax <= az {
        Vec3::new(1.0, 0.0, 0.0)
    } else if ay <= az {
        Vec3::new(0.0, 1.0, 0.0)
    } else {
        Vec3::new(0.0, 0.0, 1.0)
    };
    let tangent = n.cross(axis).normalized()?;
    Some((tangent, n.cross(tangent)))
}

/// Projects `face` (number `index`, for error reports) onto luxels of about
/// `luxel_size` world units.
pub fn project_face(
    face: &LightFace,
    index: usize,
    luxel_size: f32,
) -> Result<FaceProjection, LightmapError> {
    if !face.normal.is_finite() || face.vertices.iter().any(|v| !v.is_finite()) {
        return Err(LightmapError::NonFiniteFace(index));
    }
    let degenerate = LightmapError::DegenerateFace(index);
    if face.vertices.len() < 3 {
        return Err(degenerate);
    }
    let normal = face.normal.normalized().ok_or(degenerate)?;
    let (tangent, bitangent) = tangent_frame(normal).ok_or(degenerate)?;
    let origin = face.vertices[0];

    let (mut lo, mut hi) = ([f32::MAX; 2], [f32::MIN; 2]);
    for &p in &face.vertices {
        let d = p - origin;
        for (k, c) in [d.dot(tangent), d.dot(bitangent)].into_iter().enumerate() {
            lo[k] = lo[k].min(c);
            hi[k] = hi[k].max(c);
        }
    }
    let extent = [hi[0] - lo[0], hi[1] - lo[1]];
    if !(extent[0].is_finite() && extent[1].is_finite()) || extent[0].max(extent[1]) <= 0.0 {
        return Err(degenerate);
    }
    // The small factor keeps ceil() from rounding a full grid up by one.
    let coarsest = extent[0].max(extent[1]) / MAX_FACE_LUXELS as f32 * 1.0001;
    let size = luxel_size.max(coarsest);
    let count = |e: f32| ((e / size).ceil() as u32).clamp(1, MAX_FACE_LUXELS);
    Ok(FaceProjection {
        origin,
        tangent,
        bitangent,
        normal,
        u_min: lo[0],
        v_min: lo[1],
        luxel_size: size,
        width: count(extent[0]),
        height: count(extent[1]),
    })
}
