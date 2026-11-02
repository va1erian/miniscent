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
//!   whose corners disagree on texture, or that carry the [`solid`] spots.

pub(crate) mod blend;
pub(crate) mod fill;
mod flood;
pub(crate) mod list;
pub use list::MAX_VOXEL_TRIS;
pub(crate) mod shader;
mod solid;
mod tri;

use std::collections::HashMap;
use std::sync::Arc;

use mscent_math::Vec3;
use mscent_world::texinfo::DEFAULT_SCALE;
use mscent_world::voxel::{
    ChunkMesh, Lod, NO_CHUNK, RemeshedChunk, VoxelChunk, VoxelVertex, VoxelWorld,
};

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

/// How far a new cell can be from the light it inherits and take all of it.
const INHERIT_CELLS: i32 = 2;
/// What is left of that light farther in.
const DEEP_LIGHT: f32 = 0.6;

/// Texels per world unit of a voxel surface.
pub(crate) const TEXELS_PER_UNIT: f32 = 1.0 / DEFAULT_SCALE;

/// How far ahead the dominant axis of a normal must be, as a share of itself, for
/// a surface to take one projection alone. Inside this band two fade together.
pub(crate) const SEAM_BAND: f32 = 0.12;
/// A vertex's axis when it lies in that band: its triangles go the blended way,
/// so that no fade is cut off at the edge of a plain triangle.
const IN_SEAM: u8 = 3;

/// The two largest parts of `n`, by axis, and how far the first leads the second.
pub(crate) fn seam_of(n: Vec3) -> (u8, u8, f32) {
    let a = [n.x.abs(), n.y.abs(), n.z.abs()];
    let first = dominant_axis(n);
    let i = usize::from(first);
    let (p, q) = ((i + 1) % 3, (i + 2) % 3);
    let second = if a[p] >= a[q] { p } else { q };
    let lead = (a[i] - a[second]) / a[i].max(1e-6);
    (first, second as u8, lead)
}

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
            .map(|v| {
                let (first, _, lead) = seam_of(v.normal);
                if lead < SEAM_BAND { IN_SEAM } else { first }
            })
            .collect();
        let dir_t = mesh
            .vertices
            .iter()
            .zip(&light)
            .map(|(v, l)| tangent_dir(v, dominant_axis(v.normal), l.dir))
            .collect();
        Self {
            material: vertex_materials(mesh),
            axis,
            light,
            dir_t,
        }
    }
}

/// A cell and the light it holds.
type CellLight = ([i8; 3], VertexLight);

/// A chunk's drawing data at every level, from the light of its full detail.
fn lod_data(chunk: &VoxelChunk, full: Vec<VertexLight>) -> [LodData; 3] {
    let half = merged_light(&chunk.lods[0], &full, &chunk.lods[1], Lod::Half);
    let quarter = merged_light(&chunk.lods[0], &full, &chunk.lods[2], Lod::Quarter);
    [
        LodData::new(&chunk.lods[0], full),
        LodData::new(&chunk.lods[1], half),
        LodData::new(&chunk.lods[2], quarter),
    ]
}

/// The light of a reduced mesh: each merged vertex the mean of the cells it stands
/// for. One cell's record alone could be a dark one among lit, and a vertex this
/// far apart from the next would show it as a black block. A border vertex stands
/// for itself, so neighbouring chunks still agree at any level.
fn merged_light(
    full: &ChunkMesh,
    light: &[VertexLight],
    reduced: &ChunkMesh,
    lod: Lod,
) -> Vec<VertexLight> {
    let mut sums: HashMap<[i32; 3], (Vec3, Vec3, f32)> = HashMap::new();
    for (v, l) in full.vertices.iter().zip(light) {
        let slot = sums
            .entry(lod.key(v.cell))
            .or_insert((Vec3::ZERO, Vec3::ZERO, 0.0));
        slot.0 = slot.0 + l.color;
        slot.1 = slot.1 + l.dir;
        slot.2 += 1.0;
    }
    reduced
        .vertices
        .iter()
        .map(|v| {
            sums.get(&lod.key(v.cell))
                .map_or_else(VertexLight::default, |&(color, dir, n)| VertexLight {
                    color: color * (1.0 / n),
                    dir: dir.normalized().unwrap_or(v.normal),
                })
        })
        .collect()
}

/// Light for a remeshed chunk from what its cells held before.
fn inherit(
    mesh: &ChunkMesh,
    old: &[CellLight],
    fresh: &impl Fn(&ChunkMesh) -> Vec<VertexLight>,
) -> Vec<VertexLight> {
    let lit = fresh(mesh);
    mesh.vertices
        .iter()
        .zip(lit)
        .map(|(v, lit)| nearest_light(v.cell, old).unwrap_or(lit))
        .collect()
}

/// The light of the nearest old cell. Past [`INHERIT_CELLS`] the cell is deep in
/// something new, where less of the old light reaches.
fn nearest_light(cell: [i8; 3], old: &[CellLight]) -> Option<VertexLight> {
    let gap = |c: &[i8; 3]| {
        (0..3)
            .map(|a| (i32::from(c[a]) - i32::from(cell[a])).abs())
            .max()
            .unwrap_or(0)
    };
    let (d, mut light) = old
        .iter()
        .map(|(c, l)| (gap(c), *l))
        .min_by_key(|&(d, _)| d)?;
    if d > INHERIT_CELLS {
        light.color = light.color * DEEP_LIGHT;
    }
    Some(light)
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
    /// `light(chunk, mesh)` gives the light of a chunk's full-detail vertices. A
    /// list of the wrong length is cut or filled with darkness, never trusted.
    pub fn new(
        world: VoxelWorld,
        leaf_count: usize,
        mut light: impl FnMut(usize, &ChunkMesh) -> Vec<VertexLight>,
    ) -> Self {
        let lods = world
            .chunks
            .iter()
            .enumerate()
            .map(|(c, chunk): (usize, &Arc<VoxelChunk>)| {
                let mut full = light(c, &chunk.lods[0]);
                full.resize(chunk.lods[0].vertices.len(), VertexLight::default());
                lod_data(chunk, full)
            })
            .collect();
        let mut scene = Self {
            world,
            lods,
            leaf_chunks: vec![Vec::new(); leaf_count],
            place: Vec::new(),
        };
        scene.index();
        scene
    }

    /// Which leaf sees which chunk, and where each chunk is in its volume.
    fn index(&mut self) {
        self.leaf_chunks.iter_mut().for_each(Vec::clear);
        for (c, chunk) in (0u32..).zip(&self.world.chunks) {
            for leaf in &chunk.leaves {
                if let Some(list) = self.leaf_chunks.get_mut(leaf.0 as usize) {
                    list.push(c);
                }
            }
        }
        self.place.clear();
        self.place.resize(self.world.chunks.len(), (0, 0));
        for (v, volume) in (0u32..).zip(&self.world.volumes) {
            for (i, &c) in (0u32..).zip(&volume.table) {
                if let Some(slot) = self.place.get_mut(c as usize) {
                    *slot = (v, i);
                }
            }
        }
    }

    /// Puts remeshed chunks in place. Light that a cell already has stays, a new
    /// cell inherits from the nearest lit cell within two cells, and `fresh` lights
    /// the rest. Nothing is traced here, so a stroke's light cannot flash.
    pub fn apply(
        &mut self,
        list: Vec<RemeshedChunk>,
        fresh: impl Fn(&ChunkMesh) -> Vec<VertexLight>,
    ) {
        // By chunk number, as those numbers stand before the change.
        let mut kept: Vec<(u32, Vec<CellLight>)> = Vec::with_capacity(list.len());
        for r in &list {
            let Some(id) = self.chunk_at(r.volume, r.coord) else {
                continue;
            };
            kept.push((id, self.cell_light(id as usize)));
        }
        let changed = self.world.apply(list);
        self.lods
            .resize_with(self.world.chunks.len(), Default::default);
        for c in changed {
            let i = c as usize;
            let old = kept
                .iter()
                .find(|(id, _)| *id == c)
                .map(|(_, l)| l.as_slice());
            let Some(chunk) = self.world.chunks.get(i) else {
                continue;
            };
            let mesh = &chunk.lods[0];
            self.lods[i] = lod_data(chunk, inherit(mesh, old.unwrap_or(&[]), &fresh));
        }
        self.index();
    }

    /// The same scene over a remade `world`. A surface that did not move keeps its
    /// light, found by where its vertices are, so an add volume growing does not
    /// disturb the rest; `fresh` lights what is new.
    pub fn remade(
        &self,
        world: VoxelWorld,
        leaf_count: usize,
        fresh: impl Fn(&ChunkMesh) -> Vec<VertexLight>,
    ) -> Self {
        let key = |p: Vec3| [p.x.to_bits(), p.y.to_bits(), p.z.to_bits()];
        let mut old = HashMap::new();
        for (chunk, data) in self.world.chunks.iter().zip(&self.lods) {
            for (v, l) in chunk.lods[0].vertices.iter().zip(&data[0].light) {
                old.insert(key(v.pos), *l);
            }
        }
        Self::new(world, leaf_count, |_, mesh| {
            let lamps = fresh(mesh);
            mesh.vertices
                .iter()
                .zip(lamps)
                .map(|(v, lamp)| old.get(&key(v.pos)).copied().unwrap_or(lamp))
                .collect()
        })
    }

    fn chunk_at(&self, volume: usize, coord: [u16; 3]) -> Option<u32> {
        let v = self.world.volumes.get(volume)?;
        let i = v.index(coord.map(i32::from))?;
        v.table.get(i).copied().filter(|&c| c != NO_CHUNK)
    }

    /// The light of a chunk's cells.
    fn cell_light(&self, c: usize) -> Vec<CellLight> {
        let (Some(chunk), Some(data)) = (self.world.chunks.get(c), self.lods.get(c)) else {
            return Vec::new();
        };
        let cells = chunk.lods[0].vertices.iter().map(|v| v.cell);
        cells.zip(data[0].light.iter().copied()).collect()
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
#[cfg(test)]
mod look_tests;
#[cfg(test)]
mod shade_tests;
