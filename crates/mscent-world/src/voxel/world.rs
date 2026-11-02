//! Every volume of a level, meshed against its compiled BSP: what the renderer
//! draws and the collision sweeps. Triangles no air can see are gone by here, so
//! they cost neither light nor a pixel test, and each chunk knows the BSP leaves
//! it can be seen from.

use std::sync::Arc;

use mscent_math::epsilon::VOXEL_BURIED_MARGIN;
use mscent_math::{Aabb, Vec3};

use super::connectivity::AirMask;
use super::context::Context;
use super::lod::Lod;
use super::surface::{ChunkMesh, Mesher, VoxelVertex};
use super::volume::Volume;
use crate::brush::{Brush, Op};
use crate::bsp::{Bsp, Child, LeafId, LeafKind};
use crate::entity::Level;

/// Cells past its box a volume's surface can reach: the two rows of its ring.
const RING_REACH_CELLS: f64 = 3.0;

/// Marks a chunk of a volume's table that has no surface.
pub const NO_CHUNK: u32 = u32::MAX;

/// One chunk's surface at each level of detail, and where it can be seen from.
#[derive(Clone, Debug, PartialEq)]
pub struct VoxelChunk {
    /// Full, half and quarter detail.
    pub lods: [ChunkMesh; 3],
    /// Empty leaves its triangles touch, sorted and unique.
    pub leaves: Vec<LeafId>,
}

/// The chunks of one volume as a grid, for flooding through its air.
#[derive(Clone, Debug, PartialEq)]
pub struct VolumeGeometry {
    pub chunks: [u16; 3],
    /// Air connectivity of each chunk.
    pub air: Vec<AirMask>,
    /// Index of each chunk in [`VoxelWorld::chunks`], or [`NO_CHUNK`].
    pub table: Vec<u32>,
    /// World position of chunk `[0, 0, 0]`'s first sample.
    pub origin: Vec3,
    /// World size of a chunk.
    pub chunk_size: f32,
}

impl VolumeGeometry {
    /// Index in [`Self::table`] and [`Self::air`] of the chunk at `c`.
    pub fn index(&self, c: [i32; 3]) -> Option<usize> {
        let mut index = 0usize;
        let mut stride = 1usize;
        for (&at, &count) in c.iter().zip(&self.chunks) {
            let count = usize::from(count);
            index += usize::try_from(at).ok().filter(|&v| v < count)? * stride;
            stride *= count;
        }
        Some(index)
    }
}

/// All voxel surfaces of a level.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct VoxelWorld {
    pub chunks: Vec<Arc<VoxelChunk>>,
    pub volumes: Vec<VolumeGeometry>,
}

impl VoxelWorld {
    /// Meshes `level`'s volumes. `brushes` are its static brushes, and `bsp` was
    /// compiled from those and the shells.
    pub fn build(level: &Level, brushes: &[Brush], bsp: &Bsp) -> Self {
        let mut world = Self::default();
        let mut mesher = Mesher::new();
        for volume in &level.volumes {
            let mut ctx = Context::in_level(volume, brushes, &level.volumes);
            let geometry = world.add_volume(volume, &mut ctx, &mut mesher, bsp);
            world.volumes.push(geometry);
        }
        world
    }

    fn add_volume(
        &mut self,
        volume: &Volume,
        ctx: &mut Context,
        mesher: &mut Mesher,
        bsp: &Bsp,
    ) -> VolumeGeometry {
        let [cx, cy, cz] = volume.chunks();
        let count = usize::from(cx) * usize::from(cy) * usize::from(cz);
        let (mut air, mut table) = (Vec::with_capacity(count), Vec::with_capacity(count));
        let margin = buried_margin(volume);
        for z in 0..cz {
            for y in 0..cy {
                for x in 0..cx {
                    let mesh = mesher.mesh(volume, ctx, [x, y, z]);
                    air.push(mesh.as_ref().map_or(AirMask::NONE, |m| m.air));
                    table.push(mesh.map_or(NO_CHUNK, |m| self.add_chunk(m, bsp, margin)));
                }
            }
        }
        let (lo, _) = volume.bounds();
        VolumeGeometry {
            chunks: volume.chunks(),
            air,
            table,
            origin: lo.to_f32(),
            chunk_size: (volume.cell().units() * super::CHUNK_EDGE as f64) as f32,
        }
    }

    fn add_chunk(&mut self, mesh: ChunkMesh, bsp: &Bsp, margin: f64) -> u32 {
        let mut leaves = Vec::new();
        let full = without_buried(&mesh, bsp, margin, &mut leaves);
        if full.triangles.is_empty() {
            return NO_CHUNK;
        }
        leaves.sort_unstable_by_key(|l: &LeafId| l.0);
        leaves.dedup();
        let index = self.chunks.len() as u32;
        let lods = [full.clustered(Lod::Half), full.clustered(Lod::Quarter)];
        let [half, quarter] = lods;
        self.chunks.push(Arc::new(VoxelChunk {
            lods: [full, half, quarter],
            leaves,
        }));
        index
    }
}

/// How far from an empty leaf a triangle of `volume` still counts as seen from it.
/// A cave's surface may close just past its box, in what the BSP calls solid, with
/// its air side in the box: only an add volume's triangles are really buried there.
fn buried_margin(volume: &Volume) -> f64 {
    match volume.op() {
        Op::Add => VOXEL_BURIED_MARGIN,
        Op::Subtract => RING_REACH_CELLS * volume.cell().units(),
    }
}

/// `mesh` with only the triangles air can see, and only their vertices. Adds the
/// empty leaves those triangles touch to `leaves`.
fn without_buried(mesh: &ChunkMesh, bsp: &Bsp, margin: f64, leaves: &mut Vec<LeafId>) -> ChunkMesh {
    const UNUSED: u16 = u16::MAX;
    let mut remap = vec![UNUSED; mesh.vertices.len()];
    let mut vertices: Vec<VoxelVertex> = Vec::with_capacity(mesh.vertices.len());
    let mut triangles = Vec::with_capacity(mesh.triangles.len());
    let mut materials = Vec::with_capacity(mesh.materials.len());
    for (t, &material) in mesh.triangles.iter().zip(&mesh.materials) {
        let corners = t.map(|i| mesh.vertices[usize::from(i)].pos);
        if !touch_air(bsp, bsp.root(), &corners, margin, leaves) {
            continue;
        }
        triangles.push(t.map(|i| {
            let slot = &mut remap[usize::from(i)];
            if *slot == UNUSED {
                *slot = vertices.len() as u16;
                vertices.push(mesh.vertices[usize::from(i)]);
            }
            *slot
        }));
        materials.push(material);
    }
    ChunkMesh {
        bounds: vertices
            .iter()
            .fold(Aabb::empty(), |b, v| b.including(v.pos)),
        vertices,
        triangles,
        materials,
        air: mesh.air,
    }
}

/// Whether a triangle reaches an empty leaf below `child`, adding each it reaches
/// to `leaves`. Conservative: a triangle crossing a plane goes down both sides
/// unclipped, so it can claim a leaf it misses, which is only a little more work.
fn touch_air(
    bsp: &Bsp,
    child: Child,
    corners: &[Vec3; 3],
    margin: f64,
    leaves: &mut Vec<LeafId>,
) -> bool {
    match child {
        Child::Leaf(id) => {
            let air = bsp.leaf_kind(id) == LeafKind::Empty;
            if air {
                leaves.push(id);
            }
            air
        }
        Child::Node(id) => {
            let Some(node) = bsp.node(id) else {
                return false;
            };
            let side = corners.map(|c| node.plane.signed_distance(c.to_f64()));
            let mut air = false;
            // Depth is bounded by `Bsp::from_parts`, so recursion is shallow.
            if side.iter().any(|&d| d >= -margin) {
                air |= touch_air(bsp, node.front, corners, margin, leaves);
            }
            if side.iter().any(|&d| d <= margin) {
                air |= touch_air(bsp, node.back, corners, margin, leaves);
            }
            air
        }
    }
}
