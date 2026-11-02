//! Voxel surfaces beside the brush faces: a triangle path of its own, because
//! `draw_polygon` allocates per call and culls no back faces, which is fine for
//! hundreds of faces and not for tens of thousands (design in `docs/voxels.md`).
//!
//! A frame lists once and fills per band, as brush faces do:
//!
//! * [`list`]: which chunks, at which level of detail, with their vertices
//!   projected. [`flood`] prunes what a cave's own rock hides.
//! * [`fill`]: back-face rejection and a fill whose shared edges cannot leak or
//!   double-draw. [`shader`] lights the plain case and [`blend`] the triangles
//!   whose corners disagree on texture.

pub(crate) mod blend;
pub(crate) mod fill;
mod flood;
pub(crate) mod list;
pub(crate) mod shader;
mod tri;

use std::sync::Arc;

use mscent_math::Vec3;
use mscent_world::texinfo::DEFAULT_SCALE;
use mscent_world::voxel::{ChunkMesh, VoxelChunk, VoxelVertex, VoxelWorld};

/// Light at one vertex.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct VertexLight {
    /// Colour in shader units (256 = the plain texture colour).
    pub color: Vec3,
    /// Direction the light comes from, in world space, not normalized.
    pub dir: Vec3,
}

/// What drawing a vertex needs beyond its mesh.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct LodData {
    /// Which world axis textures each vertex: 0, 1 or 2.
    pub axis: Vec<u8>,
    /// The material each vertex is textured with.
    pub material: Vec<u8>,
    pub light: Vec<VertexLight>,
    /// The light direction in the frame the normal maps are authored in.
    pub dir_t: Vec<Vec3>,
}

/// The texture axes of each dominant world axis. The same as the default brush
/// mapping (a test holds them to it), so a voxel wall carries on the texture of
/// the brush wall it meets. A table: this runs once per triangle.
const AXES: [(Vec3, Vec3); 3] = [
    (Vec3::new(0.0, 1.0, 0.0), Vec3::new(0.0, 0.0, 1.0)),
    (Vec3::new(1.0, 0.0, 0.0), Vec3::new(0.0, 0.0, 1.0)),
    (Vec3::new(1.0, 0.0, 0.0), Vec3::new(0.0, 1.0, 0.0)),
];

pub(crate) fn axis_map(axis: u8) -> (Vec3, Vec3) {
    AXES[usize::from(axis.min(2))]
}

/// Texels per world unit of a voxel surface.
pub(crate) const TEXELS_PER_UNIT: f32 = 1.0 / DEFAULT_SCALE;

/// The axis that textures a surface facing `n`.
fn dominant_axis(n: Vec3) -> u8 {
    let (ax, ay, az) = (n.x.abs(), n.y.abs(), n.z.abs());
    if ax > az && ax >= ay {
        0
    } else if ay > az && ay > ax {
        1
    } else {
        2
    }
}

/// Each vertex's material: that of the first triangle using it.
fn vertex_materials(mesh: &ChunkMesh) -> Vec<u8> {
    let mut out = vec![None; mesh.vertices.len()];
    for (t, &m) in mesh.triangles.iter().zip(&mesh.materials) {
        for &i in t {
            if let Some(slot) = out.get_mut(usize::from(i)) {
                slot.get_or_insert(m);
            }
        }
    }
    out.into_iter().map(|m| m.unwrap_or(0)).collect()
}

fn tangent_dir(v: &VoxelVertex, axis: u8, dir: Vec3) -> Vec3 {
    let (u, w) = axis_map(axis);
    Vec3::new(dir.dot(u), -dir.dot(w), dir.dot(v.normal))
}

impl LodData {
    fn new(mesh: &ChunkMesh, light: Vec<VertexLight>) -> Self {
        let axis: Vec<u8> = mesh
            .vertices
            .iter()
            .map(|v| dominant_axis(v.normal))
            .collect();
        let dir_t = mesh
            .vertices
            .iter()
            .zip(&axis)
            .zip(&light)
            .map(|((v, &a), l)| tangent_dir(v, a, l.dir))
            .collect();
        Self {
            material: vertex_materials(mesh),
            axis,
            light,
            dir_t,
        }
    }
}

/// A level's voxel surfaces as the renderer keeps them.
#[derive(Clone, Debug, Default)]
pub struct VoxelScene {
    pub(crate) world: VoxelWorld,
    pub(crate) lods: Vec<[LodData; 3]>,
    /// For each BSP leaf, the chunks seen from it.
    pub(crate) leaf_chunks: Vec<Vec<u32>>,
    /// For each chunk, its volume and its place in that volume's grid.
    pub(crate) place: Vec<(u32, u32)>,
}

impl VoxelScene {
    /// `light(chunk, level, mesh)` gives the light of a mesh's vertices. A list
    /// of the wrong length is cut or filled with darkness, never trusted.
    pub fn new(
        world: VoxelWorld,
        leaf_count: usize,
        mut light: impl FnMut(usize, usize, &ChunkMesh) -> Vec<VertexLight>,
    ) -> Self {
        let lods = world
            .chunks
            .iter()
            .enumerate()
            .map(|(c, chunk): (usize, &Arc<VoxelChunk>)| {
                [0, 1, 2].map(|level| {
                    let mesh = &chunk.lods[level];
                    let mut l = light(c, level, mesh);
                    l.resize(mesh.vertices.len(), VertexLight::default());
                    LodData::new(mesh, l)
                })
            })
            .collect();
        let mut leaf_chunks = vec![Vec::new(); leaf_count];
        for (c, chunk) in (0u32..).zip(&world.chunks) {
            for leaf in &chunk.leaves {
                if let Some(list) = leaf_chunks.get_mut(leaf.0 as usize) {
                    list.push(c);
                }
            }
        }
        let mut place = vec![(0, 0); world.chunks.len()];
        for (v, volume) in (0u32..).zip(&world.volumes) {
            for (i, &c) in (0u32..).zip(&volume.table) {
                if let Some(slot) = place.get_mut(c as usize) {
                    *slot = (v, i);
                }
            }
        }
        Self {
            world,
            lods,
            leaf_chunks,
            place,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.world.chunks.is_empty()
    }

    pub fn world(&self) -> &VoxelWorld {
        &self.world
    }
}

#[cfg(test)]
mod fill_tests;
#[cfg(test)]
mod flood_tests;
