//! What a point on a wall is made of, for effects. Runs per shot, not per
//! particle, so it can afford to look through a leaf's faces.

use mscent_math::epsilon::{FACE_MATCH_COS, FACE_MATCH_DISTANCE, SURFACE_PROBE};
use mscent_math::{Vec3, Vec3d};
use mscent_world::Face;

use super::Scene;
use crate::texture::Texture;

/// The surface at a point: its material id and mean colour.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Surface {
    pub material: u16,
    /// `0xffRRGGBB` mean albedo of the material's texture.
    pub tint: u32,
}

/// Mean colour of a texture: the last mip when it has them, otherwise the
/// average of its texels.
pub(super) fn mean_tint(tex: &Texture) -> u32 {
    let last = tex.mip_count();
    if last > 0 {
        return tex.texel_lod(0, 0, last) | 0xff00_0000;
    }
    let (w, h) = (tex.width() as i32, tex.height() as i32);
    let mut sum = [0u64; 3];
    for (x, y) in (0..h).flat_map(|y| (0..w).map(move |x| (x, y))) {
        let t = tex.texel(x, y);
        sum[0] += u64::from((t >> 16) & 0xff);
        sum[1] += u64::from((t >> 8) & 0xff);
        sum[2] += u64::from(t & 0xff);
    }
    let n = u64::from(tex.width()) * u64::from(tex.height());
    let mean = |c: u64| (c / n.max(1)) as u32;
    0xff00_0000 | mean(sum[0]) << 16 | mean(sum[1]) << 8 | mean(sum[2])
}

/// True when `p`, which lies on the face's plane, is inside its polygon.
fn contains(face: &Face, p: Vec3d) -> bool {
    let verts = face.polygon.vertices();
    let n = face.plane.normal;
    let mut prev = verts[verts.len() - 1];
    verts.iter().all(|&v| {
        let inside = n.cross(v - prev).dot(p - prev) >= 0.0;
        prev = v;
        inside
    })
}

impl Scene {
    /// The surface a shot struck at `point` with unit `normal`: the leaf just
    /// off the wall, then its faces on that plane. `None` when no face is
    /// there, including for values that trace back to bad input.
    pub fn surface_at(&self, point: Vec3, normal: Vec3) -> Option<Surface> {
        let finite = |v: Vec3| v.x.is_finite() && v.y.is_finite() && v.z.is_finite();
        if !finite(point) || !finite(normal) {
            return None;
        }
        let leaf = self.bsp.leaf_at_f32(point + normal * SURFACE_PROBE);
        let (p, n) = (point.to_f64(), normal.to_f64());
        let list = self.leaf_faces.get(leaf.0 as usize)?;
        let on_plane = list.iter().filter_map(|&i| {
            let face = self.faces.get(i as usize)?;
            let facing = face.plane.normal.dot(n) >= f64::from(FACE_MATCH_COS);
            let dist = face.plane.signed_distance(p);
            (facing && dist.abs() <= f64::from(FACE_MATCH_DISTANCE)).then_some((face, dist))
        });
        // Coplanar faces of different materials can share a leaf. A point just
        // past every edge, from rounding, still takes the first on the plane.
        let mut first = None;
        for (face, dist) in on_plane {
            if contains(face, p - face.plane.normal * dist) {
                return Some(self.surface_of(face));
            }
            first = first.or(Some(face));
        }
        first.map(|face| self.surface_of(face))
    }

    fn surface_of(&self, face: &Face) -> Surface {
        Surface {
            material: face.material,
            tint: mean_tint(self.texture_of(face.material)),
        }
    }
}

#[cfg(test)]
#[path = "surface_tests.rs"]
mod surface_tests;
