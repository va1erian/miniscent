//! The front-to-back BSP walk that issues the draw calls.

use mscent_math::{Aabb, Vec3};
use mscent_world::{Child, Face, LeafId};

use super::Scene;
use crate::camera::{Camera, Frustum};
use crate::framebuffer::Framebuffer;
use crate::raster::draw_polygon;
use crate::shade::SpanShader;
use crate::texture::Texture;

pub(super) struct Walk<'a, F> {
    scene: &'a Scene,
    fb: &'a mut Framebuffer,
    cam: &'a Camera,
    frustum: Frustum,
    eye: Vec3,
    visible: Vec<bool>,
    drawn: Vec<bool>,
    texture: Option<&'a Texture>,
    shader_for: &'a mut F,
    stats: super::SceneStats,
}

impl<'a, S, F> Walk<'a, F>
where
    S: SpanShader,
    F: FnMut(usize, &Face) -> S,
{
    pub(super) fn new(
        scene: &'a Scene,
        fb: &'a mut Framebuffer,
        cam: &'a Camera,
        eye: Vec3,
        visible: Vec<bool>,
        texture: Option<&'a Texture>,
        shader_for: &'a mut F,
    ) -> Self {
        let stats = super::SceneStats {
            leaves_visible: visible.iter().filter(|&&v| v).count() as u32,
            leaves_total: visible.len() as u32,
            ..Default::default()
        };
        Self {
            scene,
            fb,
            cam,
            frustum: cam.frustum(),
            eye,
            visible,
            drawn: vec![false; scene.faces.len()],
            texture,
            shader_for,
            stats,
        }
    }

    pub(super) fn finish(self) -> super::SceneStats {
        self.stats
    }

    /// Visit `child` and everything below it, nearest side first.
    pub(super) fn visit(&mut self, child: Child) {
        match child {
            Child::Leaf(id) => self.draw_leaf(id),
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

    fn draw_leaf(&mut self, leaf: LeafId) {
        if !self.visible.get(leaf.0 as usize).copied().unwrap_or(false) {
            return;
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
            self.draw_face(fi);
        }
    }

    fn draw_face(&mut self, fi: u32) {
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
        let mut shader = (self.shader_for)(fi as usize, face);
        let st = draw_polygon(self.fb, self.cam, poly, tex, &mut shader);
        self.stats.pixels += st.pixels;
        self.stats.spans += st.spans;
        self.stats.drawn.push(fi);
    }
}
