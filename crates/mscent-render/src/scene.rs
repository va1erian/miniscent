//! Level rendering: portal flood for visibility, BSP walk for order.
//!
//! A frame runs in three steps:
//!
//! 1. Find the leaf holding the eye and flood through portals to collect the
//!    leaves that can be seen ([`visible`]).
//! 2. Walk the BSP front to back from the eye, skipping subtrees whose
//!    bounds are outside the view frustum.
//! 3. At each visible leaf, take its faces that face the eye, each face at
//!    most once per frame even when it borders several leaves. This far is
//!    [`Scene::visible_set`], which any renderer can use.
//! 4. Clip and project those faces, and fill them. Only this step depends on the rows being drawn,
//!    so a frame in parallel bands lists once and fills per band.
//!
//! Front-to-back order does not change the picture (the z-buffer decides) but
//! it makes the nearest surface win ties and keeps overdraw low once a shader
//! can skip covered work. When the eye is inside solid space there is no
//! sensible portal flood, so every leaf counts as visible.

mod holes;
mod pan;
mod prep;
mod reach;
mod surface;
mod visible;
mod walk;

pub use reach::LightReach;
pub use surface::Surface;

use mscent_math::{Aabb, DPlane, Vec3};
use mscent_world::{Bsp, Face, Islands, LeafId, LeafKind, Portals};

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
    /// Pixels drawn through viewport faces; `pixels` counts them too.
    pub vista_pixels: u32,
    /// Faces listed for the layers of the vistas in view.
    pub vista_faces: u32,
    /// Voxel triangles submitted for the layers of the vistas in view.
    pub vista_triangles: u32,
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
    holes: holes::Holes,
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
        let holes = holes::Holes::default();
        let node_bounds = scene_bounds(&bsp, &leaf_faces, &polys, &voxels, &holes);
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
            holes,
        }
    }

    /// This scene with viewport faces, each paired with its slot. Like voxels,
    /// they count towards the bounds of their leaves: a room walled only by them
    /// has no faces of its own, and the walk would cull it.
    /// Whether any face of the level is a viewport.
    pub fn has_viewports(&self) -> bool {
        !self.holes.polys.is_empty()
    }

    pub(crate) fn with_holes(mut self, faces: &[(Face, u8)]) -> Self {
        self.holes = holes::Holes::new(self.bsp.leaves().len(), faces);
        self.refresh_bounds();
        self
    }

    fn refresh_bounds(&mut self) {
        self.node_bounds = scene_bounds(
            &self.bsp,
            &self.leaf_faces,
            &self.polys,
            &self.voxels,
            &self.holes,
        );
    }

    pub fn voxels(&self) -> &VoxelScene {
        &self.voxels
    }

    /// Changes the voxel surfaces under a level whose brushes stay as they are.
    pub(crate) fn update_voxels(&mut self, change: impl FnOnce(&mut VoxelScene, &Bsp)) {
        change(&mut self.voxels, &self.bsp);
        self.refresh_bounds();
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
        let lists = walk::Lists {
            faces: Some(list),
            chunks: &mut Vec::new(),
            holes: None,
            pan: None,
        };
        self.list_all(cam, texture, lists)
    }

    /// [`Self::list_frame`], replacing `chunks` with the voxel chunks of the
    /// visible leaves, nearest first, and `holes` with the viewport faces.
    pub(crate) fn list_frame_voxels(
        &self,
        cam: &Camera,
        list: &mut PolyList,
        chunks: &mut Vec<u32>,
        holes: &mut PolyList,
    ) -> SceneStats {
        holes.clear();
        let lists = walk::Lists {
            faces: Some(list),
            chunks,
            holes: Some(holes),
            pan: None,
        };
        self.list_all(cam, None, lists)
    }

    /// What `cam` can see, with no rasterizer in it: the faces in the returned
    /// `drawn`, front to back and facing the eye, and the voxel chunks of the
    /// visible leaves in `chunks`, nearest first. A renderer does its own
    /// clipping and projection from here.
    pub fn visible_set(&self, cam: &Camera, chunks: &mut Vec<u32>) -> SceneStats {
        let lists = walk::Lists {
            faces: None,
            chunks,
            holes: None,
            pan: None,
        };
        self.list_all(cam, None, lists)
    }

    fn list_all(
        &self,
        cam: &Camera,
        texture: Option<&Texture>,
        mut lists: walk::Lists<'_>,
    ) -> SceneStats {
        if let Some(faces) = lists.faces.as_deref_mut() {
            faces.clear();
        }
        lists.chunks.clear();
        let eye = eye_of(cam);
        let leaf = self.bsp.leaf_at_f32(eye);
        let visible = if self.bsp.leaf_kind(leaf) == LeafKind::Empty {
            visible::visible_leaves(self, cam, eye, leaf)
        } else {
            vec![true; self.bsp.leaves().len()]
        };
        let mut w = walk::Walk::new(self, lists, cam, eye, visible, texture);
        w.visit(self.bsp.root());
        w.finish()
    }

    /// Lists a vista layer's faces from `cam`, replacing `list`: only leaves of
    /// `island`, seen through the screen `window`. An eye that parallax has
    /// carried out of the island sees all of it, and culling does the rest; the
    /// whole-level answer a frame gives for an eye in solid would show other rooms.
    /// `pan` is how many texels the layer's self-lit faces have scrolled, with
    /// storage for the shifted vertices; `None` lists every face as authored.
    /// `chunks` gets the voxel chunks of the visible leaves of the island, which
    /// is why another room's voxels never show.
    pub(crate) fn list_layer(
        &self,
        cam: &Camera,
        islands: &Islands,
        island: u32,
        window: [f32; 4],
        (list, chunks): (&mut PolyList, &mut Vec<u32>),
        pan: Option<([f32; 2], &mut Vec<Vertex>)>,
    ) -> SceneStats {
        list.clear();
        chunks.clear();
        let eye = eye_of(cam);
        let leaf = self.bsp.leaf_at_f32(eye);
        let mut visible = if islands.island_of(leaf) == Some(island) {
            visible::visible_through(self, cam, eye, leaf, window)
        } else {
            vec![true; self.bsp.leaves().len()]
        };
        // A flood out of budget also says "everything".
        for (id, seen) in (0u32..).zip(&mut visible) {
            *seen = *seen && islands.island_of(LeafId(id)) == Some(island);
        }
        let lists = walk::Lists {
            faces: Some(list),
            chunks,
            holes: None,
            pan: pan.map(|(texels, scratch)| pan::Pan { texels, scratch }),
        };
        let mut w = walk::Walk::new(self, lists, cam, eye, visible, None);
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

fn scene_bounds(
    bsp: &Bsp,
    leaf_faces: &[Vec<u32>],
    polys: &[Vec<Vertex>],
    voxels: &VoxelScene,
    holes: &holes::Holes,
) -> Vec<Aabb> {
    let mut boxes = prep::leaf_boxes(leaf_faces, polys);
    let hole_boxes = prep::leaf_boxes(&holes.leaf_holes, &holes.polys);
    for (slot, hole) in boxes.iter_mut().zip(hole_boxes) {
        *slot = slot.union(hole);
    }
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
/// (`p_cam = R p + t` gives `eye = -R^T t`). It reads `R` straight from the
/// transform: differences of transformed points lose it far from the origin,
/// where a vista's room may be, and the eye lands metres away.
pub fn eye_of(cam: &Camera) -> Vec3 {
    let view = cam.view();
    -(view.rotation().transpose().mul_vec(view.translation()))
}

/// Light scale (256 = full) from the face orientation: floors bright,
/// ceilings dark, walls in between with a slight tilt by axis.
pub fn orientation_light(face: &Face) -> u32 {
    let n = face.plane.normal;
    let v = 170.0 + n.z.max(0.0) * 86.0 + n.z.min(0.0) * 30.0 + n.x.abs() * 30.0;
    v.clamp(0.0, 512.0) as u32
}

#[cfg(test)]
#[path = "scene_tests.rs"]
mod scene_tests;
