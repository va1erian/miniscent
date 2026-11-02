//! The front-to-back BSP walk that finds a frame's faces, in draw order, and
//! lists them for the rasterizer when given somewhere to list them.

use mscent_math::{Aabb, Vec3};
use mscent_world::{Child, LeafId};

use super::Scene;
use super::pan::Pan;
use crate::camera::{Camera, Frustum};
use crate::raster::PolyList;
use crate::texture::Texture;

pub(super) struct Walk<'a> {
    scene: &'a Scene,
    /// `None` when only the visible set is wanted.
    list: Option<&'a mut PolyList>,
    chunks: &'a mut Vec<u32>,
    /// Where viewport faces go. A vista has none, so one seen from inside a
    /// layer is not drawn and the layer behind shows through it.
    holes: Option<&'a mut PolyList>,
    pan: Option<Pan<'a>>,
    hole_drawn: Vec<bool>,
    cam: &'a Camera,
    frustum: Frustum,
    eye: Vec3,
    visible: Vec<bool>,
    drawn: Vec<bool>,
    texture: Option<&'a Texture>,
    stats: super::SceneStats,
}

/// The output lists of a walk.
pub(super) struct Lists<'a> {
    pub(super) faces: Option<&'a mut PolyList>,
    pub(super) chunks: &'a mut Vec<u32>,
    pub(super) holes: Option<&'a mut PolyList>,
    /// Scrolls the self-lit faces of a vista layer; `None` lists them as they are.
    pub(super) pan: Option<Pan<'a>>,
}

impl<'a> Walk<'a> {
    pub(super) fn new(
        scene: &'a Scene,
        lists: Lists<'a>,
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
        // A layer's walk lists no viewport faces, so it has none to remember.
        let hole_drawn = match lists.holes {
            Some(_) => vec![false; scene.holes.polys.len()],
            None => Vec::new(),
        };
        Self {
            scene,
            list: lists.faces,
            chunks: lists.chunks,
            holes: lists.holes,
            pan: lists.pan,
            hole_drawn,
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
        self.list_holes(leaf);
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

    /// Lists the viewport faces of `leaf` that face the eye, each once.
    fn list_holes(&mut self, leaf: LeafId) {
        let Some(out) = self.holes.as_deref_mut() else {
            return;
        };
        let holes = &self.scene.holes;
        for &hi in holes.of_leaf(leaf) {
            let i = hi as usize;
            let (Some(done), Some(plane), Some(poly), Some(&slot)) = (
                self.hole_drawn.get_mut(i),
                holes.planes.get(i),
                holes.polys.get(i),
                holes.slots.get(i),
            ) else {
                continue;
            };
            if *done {
                continue;
            }
            *done = true;
            if plane.signed_distance(self.eye.to_f64()) > 0.0 {
                out.push(self.cam, poly, &self.scene.fallback, u32::from(slot));
            }
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
        if let Some(list) = self.list.as_deref_mut() {
            let tex = self
                .texture
                .unwrap_or_else(|| self.scene.texture_of(face.material));
            let size = (tex.width(), tex.height());
            let verts = match self.pan.as_mut() {
                Some(pan) => pan.apply(face.material, poly, size),
                None => poly,
            };
            list.push(self.cam, verts, tex, fi);
        }
        self.stats.drawn.push(fi);
    }
}
