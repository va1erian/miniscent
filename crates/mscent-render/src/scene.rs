//! Level rendering: portal flood for visibility, BSP walk for order.
//!
//! A frame runs in three steps:
//!
//! 1. Find the leaf holding the eye and flood through portals to collect the
//!    leaves that can be seen ([`visible`]).
//! 2. Walk the BSP front to back from the eye, skipping subtrees whose
//!    bounds are outside the view frustum.
//! 3. At each visible leaf, draw its faces that face the eye, each face at
//!    most once per frame even when it borders several leaves.
//!
//! Front-to-back order does not change the picture (the z-buffer decides) but
//! it makes the nearest surface win ties and keeps overdraw low once a shader
//! can skip covered work. When the eye is inside solid space there is no
//! sensible portal flood, so every leaf counts as visible.

mod prep;
mod visible;
mod walk;

use mscent_math::{Aabb, DPlane, Vec3};
use mscent_world::{Bsp, Face, LeafKind, Portals};

use crate::camera::Camera;
use crate::framebuffer::Framebuffer;
use crate::raster::Vertex;
use crate::shade::{FlatShader, Fog, SpanShader};
use crate::texture::{MaterialId, MaterialTable, Texture};

/// How far behind a portal plane the eye may be and still look through it,
/// in world units. Absorbs the `f32` rounding of the camera position.
const SIDE_SLACK: f64 = 1.0e-3;

/// A portal as the visibility flood needs it.
struct PortalInfo {
    plane: DPlane,
    points: Vec<Vec3>,
}

/// What one frame did. `drawn` lists face indices in draw order, which lets
/// tests prove that no face is drawn twice.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SceneStats {
    pub drawn: Vec<u32>,
    pub leaves_visible: u32,
    pub leaves_total: u32,
    pub nodes_culled: u32,
    pub pixels: u32,
    pub spans: u32,
}

/// A compiled level ready to draw.
pub struct Scene {
    bsp: Bsp,
    faces: Vec<Face>,
    portals: Portals,
    materials: MaterialTable,
    fallback: Texture,
    polys: Vec<Vec<Vertex>>,
    leaf_faces: Vec<Vec<u32>>,
    node_bounds: Vec<Aabb>,
    portal_info: Vec<PortalInfo>,
}

impl Scene {
    /// Prepare a level for drawing. Faces whose material id is not in
    /// `materials` use a grey fallback texture.
    pub fn new(bsp: Bsp, faces: Vec<Face>, portals: Portals, materials: MaterialTable) -> Self {
        let polys: Vec<Vec<Vertex>> = faces.iter().map(prep::face_vertices).collect();
        let leaf_faces = prep::leaf_faces(bsp.leaves().len(), &faces);
        let boxes = prep::leaf_boxes(&leaf_faces, &polys);
        let node_bounds = prep::node_bounds(&bsp, &boxes);
        let portal_info = portals
            .portals()
            .iter()
            .map(|p| PortalInfo {
                // A degenerate portal gets a zero plane, which the flood
                // treats as "cannot rule it out".
                plane: p
                    .polygon
                    .plane()
                    .unwrap_or(DPlane::new(mscent_math::Vec3d::ZERO, 0.0)),
                points: prep::polygon_points(&p.polygon),
            })
            .collect();
        Self {
            bsp,
            faces,
            portals,
            materials,
            fallback: Texture::solid(0xff80_8080),
            polys,
            leaf_faces,
            node_bounds,
            portal_info,
        }
    }

    pub fn bsp(&self) -> &Bsp {
        &self.bsp
    }

    pub fn faces(&self) -> &[Face] {
        &self.faces
    }

    pub fn portals(&self) -> &Portals {
        &self.portals
    }

    /// Draw the level with the flat-lit fallback shading (a fixed light per
    /// face orientation, no lightmap).
    pub fn render(&self, fb: &mut Framebuffer, cam: &Camera, fog: Option<Fog>) -> SceneStats {
        self.render_with(fb, cam, None, &mut |_, face| {
            FlatShader::new(orientation_light(face), fog)
        })
    }

    /// Draw with a caller-supplied shader per face. `texture` replaces the
    /// material textures when set (used by id-buffer debugging). The
    /// framebuffer should be cleared first; entities are drawn after this.
    pub fn render_with<S, F>(
        &self,
        fb: &mut Framebuffer,
        cam: &Camera,
        texture: Option<&Texture>,
        shader_for: &mut F,
    ) -> SceneStats
    where
        S: SpanShader,
        F: FnMut(usize, &Face) -> S,
    {
        let eye = eye_of(cam);
        let leaf = self.bsp.leaf_at_f32(eye);
        let visible = if self.bsp.leaf_kind(leaf) == LeafKind::Empty {
            visible::visible_leaves(self, cam, eye, leaf)
        } else {
            vec![true; self.bsp.leaves().len()]
        };
        let mut w = walk::Walk::new(self, fb, cam, eye, visible, texture, shader_for);
        w.visit(self.bsp.root());
        w.finish()
    }

    fn texture_of(&self, material: u16) -> &Texture {
        self.materials
            .get(MaterialId(material))
            .map_or(&self.fallback, |m| &m.texture)
    }
}

/// World position of the camera, recovered from its view transform
/// (`p_cam = R p + t` gives `eye = -R^T t`).
pub fn eye_of(cam: &Camera) -> Vec3 {
    let t = cam.world_to_camera(Vec3::ZERO);
    let col = |axis: Vec3| cam.world_to_camera(axis) - t;
    let (cx, cy, cz) = (
        col(Vec3::new(1.0, 0.0, 0.0)),
        col(Vec3::new(0.0, 1.0, 0.0)),
        col(Vec3::new(0.0, 0.0, 1.0)),
    );
    Vec3::new(-cx.dot(t), -cy.dot(t), -cz.dot(t))
}

/// Light scale (256 = full) from the face orientation: floors bright,
/// ceilings dark, walls in between with a slight tilt by axis.
fn orientation_light(face: &Face) -> u32 {
    let n = face.plane.normal;
    let v = 170.0 + n.z.max(0.0) * 86.0 + n.z.min(0.0) * 30.0 + n.x.abs() * 30.0;
    v.clamp(0.0, 512.0) as u32
}

#[cfg(test)]
#[path = "scene_tests.rs"]
mod scene_tests;
