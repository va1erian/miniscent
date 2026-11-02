//! Level rendering: portal flood for visibility, BSP walk for order.
//!
//! A frame runs in three steps:
//!
//! 1. Find the leaf holding the eye and flood through portals to collect the
//!    leaves that can be seen ([`visible`]).
//! 2. Walk the BSP front to back from the eye, skipping subtrees whose
//!    bounds are outside the view frustum.
//! 3. At each visible leaf, list its faces that face the eye, each face at
//!    most once per frame even when it borders several leaves.
//! 4. Fill the listed faces. Only this step depends on the rows being drawn,
//!    so a frame in parallel bands lists once and fills per band.
//!
//! Front-to-back order does not change the picture (the z-buffer decides) but
//! it makes the nearest surface win ties and keeps overdraw low once a shader
//! can skip covered work. When the eye is inside solid space there is no
//! sensible portal flood, so every leaf counts as visible.

mod prep;
mod reach;
mod surface;
mod visible;
mod walk;

pub use reach::LightReach;
pub use surface::Surface;

use mscent_math::{Aabb, DPlane, Vec3};
use mscent_world::{Bsp, Face, LeafKind, Portals};

use crate::camera::Camera;
use crate::framebuffer::Framebuffer;
use crate::material::{MaterialId, MaterialTable};
use crate::raster::{DrawStats, PolyList, Vertex};
use crate::shade::{FlatShader, Fog, SpanShader};
use crate::texture::Texture;
use crate::voxel::VoxelScene;

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
    /// Voxel triangles submitted.
    pub voxel_triangles: u32,
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
    voxels: VoxelScene,
}

impl Scene {
    /// Prepare a level for drawing. Faces whose material id is not in
    /// `materials` use a grey fallback texture.
    pub fn new(bsp: Bsp, faces: Vec<Face>, portals: Portals, materials: MaterialTable) -> Self {
        Self::with_voxels(bsp, faces, portals, materials, VoxelScene::default())
    }

    /// [`Self::new`] with voxel surfaces. Their chunks count towards the bounds
    /// of the leaves that see them: a volume's leaf has no faces of its own, and
    /// the walk would cull it, voxels and all.
    pub fn with_voxels(
        bsp: Bsp,
        faces: Vec<Face>,
        portals: Portals,
        materials: MaterialTable,
        voxels: VoxelScene,
    ) -> Self {
        let polys: Vec<Vec<Vertex>> = faces
            .iter()
            .map(|f| {
                let size = materials
                    .get(MaterialId(f.material))
                    .map(|m| (m.texture.width(), m.texture.height()));
                prep::face_vertices(f, size)
            })
            .collect();
        let leaf_faces = prep::leaf_faces(bsp.leaves().len(), &faces);
        let node_bounds = bounds_with_voxels(&bsp, &leaf_faces, &polys, &voxels);
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
            voxels,
        }
    }

    pub fn voxels(&self) -> &VoxelScene {
        &self.voxels
    }

    /// Changes the voxel surfaces under a level whose brushes stay as they are.
    pub(crate) fn update_voxels(&mut self, change: impl FnOnce(&mut VoxelScene)) {
        change(&mut self.voxels);
        self.node_bounds =
            bounds_with_voxels(&self.bsp, &self.leaf_faces, &self.polys, &self.voxels);
    }

    pub fn bsp(&self) -> &Bsp {
        &self.bsp
    }

    pub fn faces(&self) -> &[Face] {
        &self.faces
    }

    /// Rasterizer polygons (with texture coordinates from each face's TexInfo), one per face.
    pub fn polys(&self) -> &[Vec<Vertex>] {
        &self.polys
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
        let mut list = PolyList::default();
        let mut stats = self.list_frame(cam, texture, &mut list);
        let drawn = self.fill_frame(fb, &list, texture, shader_for);
        stats.pixels = drawn.pixels;
        stats.spans = drawn.spans;
        stats
    }

    /// The part of a frame that does not depend on the rows being drawn:
    /// visibility and the clipped, projected faces in draw order, replacing
    /// `list`. Run it once per frame, then [`Self::fill_frame`] per band. The
    /// stats lack `pixels` and `spans`, which only filling knows.
    pub(crate) fn list_frame(
        &self,
        cam: &Camera,
        texture: Option<&Texture>,
        list: &mut PolyList,
    ) -> SceneStats {
        self.list_frame_voxels(cam, texture, list, &mut Vec::new())
    }

    /// [`Self::list_frame`], replacing `chunks` with the voxel chunks of the
    /// visible leaves, nearest first.
    pub(crate) fn list_frame_voxels(
        &self,
        cam: &Camera,
        texture: Option<&Texture>,
        list: &mut PolyList,
        chunks: &mut Vec<u32>,
    ) -> SceneStats {
        list.clear();
        chunks.clear();
        let eye = eye_of(cam);
        let leaf = self.bsp.leaf_at_f32(eye);
        let visible = if self.bsp.leaf_kind(leaf) == LeafKind::Empty {
            visible::visible_leaves(self, cam, eye, leaf)
        } else {
            vec![true; self.bsp.leaves().len()]
        };
        let mut w = walk::Walk::new(self, list, chunks, cam, eye, visible, texture);
        w.visit(self.bsp.root());
        w.finish()
    }

    /// Fills the faces of `list` that touch the rows `fb` holds. `texture` must
    /// be what the list was made with.
    pub(crate) fn fill_frame<S, F>(
        &self,
        fb: &mut Framebuffer,
        list: &PolyList,
        texture: Option<&Texture>,
        shader_for: &mut F,
    ) -> DrawStats
    where
        S: SpanShader,
        F: FnMut(usize, &Face) -> S,
    {
        let mut total = DrawStats::default();
        for poly in list.in_rows(fb.row_range()) {
            let Some(face) = self.faces.get(poly.tag as usize) else {
                continue;
            };
            let tex = texture.unwrap_or_else(|| self.texture_of(face.material));
            let mut shader = shader_for(poly.tag as usize, face);
            let st = list.fill(fb, poly, tex, &mut shader);
            total.pixels += st.pixels;
            total.spans += st.spans;
        }
        total
    }

    /// Albedo texture of a material id, or the grey fallback.
    pub fn texture_of(&self, material: u16) -> &Texture {
        self.materials
            .get(MaterialId(material))
            .map_or(&self.fallback, |m| &m.texture)
    }
}

fn bounds_with_voxels(
    bsp: &Bsp,
    leaf_faces: &[Vec<u32>],
    polys: &[Vec<Vertex>],
    voxels: &VoxelScene,
) -> Vec<Aabb> {
    let mut boxes = prep::leaf_boxes(leaf_faces, polys);
    for (slot, list) in boxes.iter_mut().zip(&voxels.leaf_chunks) {
        for chunk in list
            .iter()
            .filter_map(|&c| voxels.world.chunks.get(c as usize))
        {
            *slot = slot.union(chunk.lods[0].bounds);
        }
    }
    prep::node_bounds(bsp, &boxes)
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
