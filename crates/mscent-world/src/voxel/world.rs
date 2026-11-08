//! Every volume of a level, meshed against its compiled BSP: what the renderer
//! draws and the collision sweeps. Triangles no air can see are gone by here, so
//! they cost neither light nor a pixel test, and each chunk knows the BSP leaves
//! it can be seen from.

use std::sync::Arc;

use mscent_math::epsilon::VOXEL_BURIED_MARGIN;
use mscent_math::{Aabb, Vec3};

use super::connectivity::AirMask;
use super::context::Context;
use super::index::CellIndex;
use super::lod::Lod;
use super::surface::{ChunkMesh, Mesher, VoxelVertex};
use super::volume::Volume;
use crate::brush::{Brush, Op};
use crate::bsp::{Bsp, Child, LeafId, LeafKind};
use crate::entity::Level;
use crate::lightmap::parallel::par_map;

/// Cells past its box a volume's surface can reach: the two rows of its ring.
const RING_REACH_CELLS: f64 = 3.0;

/// Marks a chunk of a volume's table that has no surface.
pub const NO_CHUNK: u32 = u32::MAX;

/// Chunks one thread meshes in a row with one mesher and one context, which
/// keep what neighbouring chunks share. Small enough that a cave's few hundred
/// chunks spread over every core.
const BAND: usize = 8;

/// What meshing one chunk gave, or `None` where its cells hold no surface: its
/// air, whether its air lies in rock, and its surface once buried triangles are
/// gone (`None` when air sees none of it).
type Meshed = Option<(AirMask, bool, Option<VoxelChunk>)>;

/// One chunk's surface at each level of detail, and where it can be seen from.
#[derive(Clone, Debug, PartialEq)]
pub struct VoxelChunk {
    /// Full, half and quarter detail.
    pub lods: [ChunkMesh; 3],
    /// Empty leaves its triangles touch, sorted and unique.
    pub leaves: Vec<LeafId>,
    /// `lods[0]`'s triangles by cell, for collision.
    pub index: CellIndex,
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
    /// Whether each chunk's air (as the BSP sees it) lies in rock, parallel to
    /// [`Self::table`]. It answers "is this point solid" where no surface is
    /// near enough to say, such as the middle of a mass of rock.
    pub rock: Vec<bool>,
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
    ///
    /// Chunks are meshed on every core: each one's surface depends only on the
    /// level, so the world is the same, chunk for chunk, on any number of them.
    pub fn build(level: &Level, brushes: &[Brush], bsp: &Bsp) -> Self {
        Self::build_on(cores(), level, brushes, bsp)
    }

    /// As [`Self::build`] on up to `workers` threads.
    pub fn build_on(workers: usize, level: &Level, brushes: &[Brush], bsp: &Bsp) -> Self {
        let mut world = Self::default();
        for volume in &level.volumes {
            let [cx, cy, cz] = volume.chunks();
            let coords: Vec<[u16; 3]> = (0..cz)
                .flat_map(|z| (0..cy).flat_map(move |y| (0..cx).map(move |x| [x, y, z])))
                .collect();
            let meshed = mesh_chunks(workers, volume, brushes, &level.volumes, bsp, &coords);
            let geometry = world.add_volume(volume, meshed);
            world.volumes.push(geometry);
        }
        world
    }

    fn add_volume(&mut self, volume: &Volume, meshed: Vec<Meshed>) -> VolumeGeometry {
        let count = meshed.len();
        let (mut air, mut table) = (Vec::with_capacity(count), Vec::with_capacity(count));
        let mut rock = Vec::with_capacity(count);
        for m in meshed {
            let Some((a, r, chunk)) = m else {
                air.push(AirMask::NONE);
                rock.push(false);
                table.push(NO_CHUNK);
                continue;
            };
            air.push(a);
            rock.push(r);
            table.push(chunk.map_or(NO_CHUNK, |c| self.add_chunk(c)));
        }
        let (lo, _) = volume.bounds();
        VolumeGeometry {
            chunks: volume.chunks(),
            air,
            table,
            origin: lo.to_f32(),
            chunk_size: (volume.cell().units() * super::CHUNK_EDGE as f64) as f32,
            rock,
        }
    }

    fn add_chunk(&mut self, chunk: VoxelChunk) -> u32 {
        let index = self.chunks.len() as u32;
        self.chunks.push(Arc::new(chunk));
        index
    }

    /// Meshes `chunks` of each listed one of `volumes` again, off to the side. The
    /// grids must be those of this world's level.
    pub fn remesh(
        volumes: &[Volume],
        brushes: &[Brush],
        bsp: &Bsp,
        wanted: &[(usize, Vec<[u16; 3]>)],
    ) -> Vec<RemeshedChunk> {
        let mut out = Vec::new();
        for (v, chunks) in wanted {
            let Some(volume) = volumes.get(*v) else {
                continue;
            };
            let meshed = mesh_chunks(cores(), volume, brushes, volumes, bsp, chunks);
            for (&coord, m) in chunks.iter().zip(meshed) {
                let Some((air, rock, chunk)) = m else {
                    continue;
                };
                out.push(RemeshedChunk {
                    volume: *v,
                    coord,
                    air,
                    rock,
                    chunk: chunk.map(Arc::new),
                });
            }
        }
        out
    }

    /// Puts remeshed chunks in place, all at once as far as a frame can tell, and
    /// says which chunk numbers changed. A chunk that lost its surface keeps its
    /// number with nothing in it, so the numbers of the others stand.
    pub fn apply(&mut self, list: Vec<RemeshedChunk>) -> Vec<u32> {
        let mut changed = Vec::with_capacity(list.len());
        for r in list {
            let Some(volume) = self.volumes.get_mut(r.volume) else {
                continue;
            };
            let Some(i) = volume.index(r.coord.map(i32::from)) else {
                continue;
            };
            if let Some(slot) = volume.air.get_mut(i) {
                *slot = r.air;
            }
            if let Some(slot) = volume.rock.get_mut(i) {
                *slot = r.rock;
            }
            let old = volume.table.get(i).copied().unwrap_or(NO_CHUNK);
            let id = match (old, r.chunk) {
                (NO_CHUNK, None) => continue,
                (NO_CHUNK, Some(chunk)) => {
                    let id = self.chunks.len() as u32;
                    self.chunks.push(chunk);
                    if let Some(slot) = volume.table.get_mut(i) {
                        *slot = id;
                    }
                    id
                }
                (id, new) => {
                    if let Some(slot) = self.chunks.get_mut(id as usize) {
                        *slot = new.unwrap_or_else(|| Arc::new(empty_chunk()));
                    }
                    id
                }
            };
            changed.push(id);
        }
        changed
    }
}

/// The threads to mesh on: one per core.
fn cores() -> usize {
    std::thread::available_parallelism().map_or(1, usize::from)
}

/// Meshes `coords` of `volume`, in their order, on up to `workers` threads.
fn mesh_chunks(
    workers: usize,
    volume: &Volume,
    brushes: &[Brush],
    volumes: &[Volume],
    bsp: &Bsp,
    coords: &[[u16; 3]],
) -> Vec<Meshed> {
    let margin = buried_margin(volume);
    let bands: Vec<&[[u16; 3]]> = coords.chunks(BAND).collect();
    let done = par_map(workers, bands.len(), |b| {
        let mut mesher = Mesher::new();
        let mut ctx = Context::in_level(volume, brushes, volumes);
        let band = bands.get(b).copied().unwrap_or_default();
        band.iter()
            .map(|&c| {
                let mesh = mesher.mesh(volume, &mut ctx, c)?;
                let rock = probe_rock(volume, &mesher, bsp, c);
                Some((mesh.air, rock, finish_chunk(&mesh, bsp, margin)))
            })
            .collect::<Vec<Meshed>>()
    });
    done.into_iter().flatten().collect()
}

/// One chunk's new surface, or `None` for a chunk with none left.
#[derive(Clone, Debug, PartialEq)]
pub struct RemeshedChunk {
    pub volume: usize,
    pub coord: [u16; 3],
    pub air: AirMask,
    /// See [`VolumeGeometry::rock`].
    pub rock: bool,
    pub chunk: Option<Arc<VoxelChunk>>,
}

fn empty_mesh() -> ChunkMesh {
    ChunkMesh {
        vertices: Vec::new(),
        triangles: Vec::new(),
        materials: Vec::new(),
        bounds: Aabb::empty(),
        air: AirMask::ALL,
    }
}

fn empty_chunk() -> VoxelChunk {
    VoxelChunk {
        index: CellIndex::build(&empty_mesh()),
        lods: [empty_mesh(), empty_mesh(), empty_mesh()],
        leaves: Vec::new(),
    }
}

/// A chunk's levels of detail and leaves, or `None` when air can see none of it.
fn finish_chunk(mesh: &ChunkMesh, bsp: &Bsp, margin: f64) -> Option<VoxelChunk> {
    let mut leaves = Vec::new();
    let full = without_buried(mesh, bsp, margin, &mut leaves);
    if full.triangles.is_empty() {
        return None;
    }
    leaves.sort_unstable_by_key(|l: &LeafId| l.0);
    leaves.dedup();
    let [half, quarter] = [full.clustered(Lod::Half), full.clustered(Lod::Quarter)];
    Some(VoxelChunk {
        index: CellIndex::build(&full),
        lods: [full, half, quarter],
        leaves,
    })
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

/// Samples of a chunk's block tried by [`probe_rock`], per axis.
const PROBES: [i32; 3] = [2, 8, 14];

/// Whether the field is solid where the BSP is air in chunk `coord`, judged at
/// the first of 27 samples the BSP leaves open. A chunk whose air is all in one
/// piece of the field, as a chunk with no surface is, is then told correctly
/// even when brush solid covers part of it. `mesher` still holds the chunk's
/// samples. A chunk with no open sample has no air to ask about.
fn probe_rock(volume: &Volume, mesher: &Mesher, bsp: &Bsp, coord: [u16; 3]) -> bool {
    let first = coord.map(|c| i32::from(c) * super::CHUNK_EDGE as i32);
    for z in PROBES {
        for y in PROBES {
            for x in PROBES {
                let local = [x, y, z];
                let at = [0, 1, 2].map(|a| first[a] + local[a]);
                if !bsp.is_solid(volume.lattice.position(at)) {
                    return mesher.solid_at(local);
                }
            }
        }
    }
    false
}
