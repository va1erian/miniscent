//! The front-to-back BSP walk that lists a frame's polygons in draw order.

use mscent_math::{Aabb, Vec3};
use mscent_world::{Child, LeafId};

use super::Scene;
use crate::camera::{Camera, Frustum};
use crate::raster::PolyList;
use crate::texture::Texture;

pub(super) struct Walk<'a> {
    scene: &'a Scene,
    list: &'a mut PolyList,
    chunks: &'a mut Vec<u32>,
    cam: &'a Camera,
    frustum: Frustum,
    eye: Vec3,
    visible: Vec<bool>,
    drawn: Vec<bool>,
    texture: Option<&'a Texture>,
    stats: super::SceneStats,
}

impl<'a> Walk<'a> {
    pub(super) fn new(
        scene: &'a Scene,
        list: &'a mut PolyList,
        chunks: &'a mut Vec<u32>,
        cam: &'a Camera,
        eye: Vec3,
        visible: Vec<bool>,
        texture: Option<&'a Texture>,
    ) -> Self {
        let stats = super::SceneStats {
            leaves_visible: visible.iter().filter(|&&v| v).count() as u32,
            leaves_total: visible.len() as u32,
            ..Default::default()
        };
        Self {
            scene,
            list,
            chunks,
            cam,
            frustum: cam.frustum(),
            eye,
            visible,
            drawn: vec![false; scene.faces.len()],
            texture,
            stats,
        }
    }

    pub(super) fn finish(self) -> super::SceneStats {
        self.stats
    }

    /// Visit `child` and everything below it, nearest side first.
    pub(super) fn visit(&mut self, child: Child) {
        match child {
            Child::Leaf(id) => self.list_leaf(id),
            Child::Node(id) => {
                let Some(node) = self.scene.bsp.node(id) else {
                    return;
                };
                let bounds = self.scene.node_bounds.get(id.0 as usize);
                if bounds.is_none_or(|b| b.is_empty() || !self.box_in_view(b)) {
                    self.stats.nodes_culled += 1;
                    return;
                }
                let eye_front = node.plane.signed_distance(self.eye.to_f64()) > 0.0;
                let (near, far) = if eye_front {
                    (node.front, node.back)
                } else {
                    (node.back, node.front)
                };
                self.visit(near);
                self.visit(far);
            }
        }
    }

    /// False only when all eight corners lie outside one frustum plane.
    fn box_in_view(&self, b: &Aabb) -> bool {
        let corners: [Vec3; 8] = std::array::from_fn(|i| {
            let pick = |bit: usize, lo: f32, hi: f32| if i & bit == 0 { lo } else { hi };
            let p = Vec3::new(
                pick(1, b.min.x, b.max.x),
                pick(2, b.min.y, b.max.y),
                pick(4, b.min.z, b.max.z),
            );
            self.cam.world_to_camera(p)
        });
        (0..6).all(|plane| {
            corners
                .iter()
                .any(|&c| self.frustum.distance(plane, c) >= 0.0)
        })
    }

    fn list_leaf(&mut self, leaf: LeafId) {
        if !self.visible.get(leaf.0 as usize).copied().unwrap_or(false) {
            return;
        }
        if let Some(list) = self.scene.voxels.leaf_chunks.get(leaf.0 as usize) {
            self.chunks.extend_from_slice(list);
        }
        let Some(list) = self.scene.leaf_faces.get(leaf.0 as usize) else {
            return;
        };
        for &fi in list {
            let slot = self.drawn.get_mut(fi as usize);
            match slot {
                Some(done) if !*done => *done = true,
                _ => continue,
            }
            self.list_face(fi);
        }
    }

    fn list_face(&mut self, fi: u32) {
        let (Some(face), Some(poly)) = (
            self.scene.faces.get(fi as usize),
            self.scene.polys.get(fi as usize),
        ) else {
            return;
        };
        // Faces point into empty space; the eye behind one sees its back.
        if face.plane.signed_distance(self.eye.to_f64()) <= 0.0 {
            return;
        }
        let tex = self
            .texture
            .unwrap_or_else(|| self.scene.texture_of(face.material));
        self.list.push(self.cam, poly, tex, fi);
        self.stats.drawn.push(fi);
    }
}
