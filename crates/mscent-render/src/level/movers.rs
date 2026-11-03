//! Drawing movers (M10.7). A mover's faces are ordinary faces of the scene, lit
//! at their authored pose, that belong to no leaf, so the BSP walk never draws
//! them. Here each is drawn wherever its mover is now, and the z-buffer sorts
//! it against the world.
//!
//! Lighting a face works in face space, so only the polygon is moved and the
//! baked light rides along with the door. Specular needs the eye in that same
//! space, so the eye is moved the other way. A closed door culls nothing behind
//! it: it costs what an open doorway costs.

use mscent_math::{Quat, Vec3};

use super::LevelRenderer;
use crate::Quality;
use crate::camera::Camera;
use crate::framebuffer::Framebuffer;
use crate::raster::{DrawStats, PolyList, Vertex};
use crate::scene::eye_of;

/// Where a mover is, relative to how it was authored: a point `x` goes to
/// `rot (x - pivot) + pivot + offset`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MoverPose {
    pub rot: Quat,
    pub pivot: Vec3,
    pub offset: Vec3,
}

impl MoverPose {
    pub const AUTHORED: Self = Self {
        rot: Quat::IDENTITY,
        pivot: Vec3::ZERO,
        offset: Vec3::ZERO,
    };

    pub fn to_world(self, p: Vec3) -> Vec3 {
        self.rot.rotate(p - self.pivot) + self.pivot + self.offset
    }

    pub fn to_authored(self, p: Vec3) -> Vec3 {
        self.rot.conjugate().rotate(p - self.pivot - self.offset) + self.pivot
    }
}

/// One mover's faces, and a sphere around them for culling.
#[derive(Clone, Debug, PartialEq)]
pub struct MoverFaces {
    pub faces: std::ops::Range<usize>,
    pub centre: Vec3,
    pub radius: f32,
}

/// This frame's movers, listed once and filled by every band, as the level is.
#[derive(Default)]
pub(super) struct MoverList {
    polys: PolyList,
    /// By a polygon's tag: its face, and the eye in the space it was authored in.
    faces: Vec<(usize, Vec3)>,
    /// Kept so a frame does not allocate it again.
    moved: Vec<Vertex>,
}

/// The sphere around a set of polygons.
pub(super) fn bounding_sphere(polys: &[Vec<Vertex>]) -> (Vec3, f32) {
    let points = || polys.iter().flatten().map(|v| v.pos);
    let bounds = points().fold(mscent_math::Aabb::empty(), |b, p| b.including(p));
    if bounds.is_empty() {
        return (Vec3::ZERO, 0.0);
    }
    let centre = bounds.center();
    let radius = points().map(|p| (p - centre).length()).fold(0.0, f32::max);
    (centre, radius)
}

impl LevelRenderer {
    /// Replaces `list` with every mover's faces, each mover at its entry in
    /// `poses` or as authored.
    pub(super) fn list_movers(&self, cam: &Camera, poses: &[MoverPose], list: &mut MoverList) {
        list.polys.clear();
        list.faces.clear();
        for (i, mover) in self.assets.mover_faces.iter().enumerate() {
            let pose = poses.get(i).copied().unwrap_or(MoverPose::AUTHORED);
            // View is along -Z, so all of it is behind the eye past this.
            if cam.world_to_camera(pose.to_world(mover.centre)).z > mover.radius {
                continue;
            }
            for face in mover.faces.clone() {
                self.list_mover_face(cam, &pose, face, list);
            }
        }
    }

    fn list_mover_face(&self, cam: &Camera, pose: &MoverPose, index: usize, list: &mut MoverList) {
        let scene = self.scene();
        let (Some(face), Some(poly)) = (scene.faces().get(index), scene.polys().get(index)) else {
            return;
        };
        let eye = pose.to_authored(eye_of(cam));
        // Faces point into empty space; the eye behind one sees its back.
        if face.plane.signed_distance(eye.to_f64()) <= 0.0 {
            return;
        }
        list.moved.clear();
        list.moved.extend(poly.iter().map(|v| Vertex {
            pos: pose.to_world(v.pos),
            uv: v.uv,
        }));
        let tag = list.faces.len() as u32;
        let texture = scene.texture_of(face.material);
        list.polys.push(cam, &list.moved, texture, tag);
        list.faces.push((index, eye));
    }

    /// Fills the listed faces that touch the rows `fb` holds.
    pub(super) fn fill_movers(
        &self,
        fb: &mut Framebuffer,
        list: &MoverList,
        quality: Quality,
    ) -> DrawStats {
        let scene = self.scene();
        let mut total = DrawStats::default();
        for poly in list.polys.in_rows(fb.row_range()) {
            let Some(&(index, eye)) = list.faces.get(poly.tag as usize) else {
                continue;
            };
            let Some(face) = scene.faces().get(index) else {
                continue;
            };
            // Lights reach faces through leaves, and a mover's belong to none.
            let mut shader = self.face_shader(index, face, eye, quality, &[], self.fog());
            let texture = scene.texture_of(face.material);
            let st = list.polys.fill(fb, poly, texture, &mut shader);
            total.pixels += st.pixels;
            total.spans += st.spans;
        }
        total
    }
}
