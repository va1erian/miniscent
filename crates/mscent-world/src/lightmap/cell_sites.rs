//! Where the light records of voxel surfaces go: the surface cells of a
//! [`VoxelWorld`], sorted by their place in their volume, with the position and
//! normal each is lit at. Which cells exist follows from the world alone, so a
//! cache file stores records and no keys.

use mscent_math::Vec3;

use super::store::{FNV_BASIS, Fnv};
use crate::voxel::{CHUNK_EDGE, NO_CHUNK, VoxelWorld};

const EDGE: i32 = CHUNK_EDGE as i32;

/// A cell of a volume. Fields are in sort order, so the cells of a chunk are
/// next to each other.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CellKey {
    pub volume: u32,
    /// The chunk that owns the cell: the one whose first sample it is nearest
    /// to. Cells in the ring past a volume's box belong to chunks outside it.
    pub chunk: [i32; 3],
    /// Cell within the owning chunk.
    pub local: [u8; 3],
}

impl CellKey {
    /// The cell at `global` cells from the volume's first sample.
    pub fn from_global(volume: u32, global: [i32; 3]) -> Self {
        Self {
            volume,
            chunk: global.map(|v| v.div_euclid(EDGE)),
            local: global.map(|v| v.rem_euclid(EDGE) as u8),
        }
    }

    pub fn global(&self) -> [i32; 3] {
        [0, 1, 2].map(|k| self.chunk[k] * EDGE + i32::from(self.local[k]))
    }
}

/// The surface cells of a world.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CellSites {
    keys: Vec<CellKey>,
    pos: Vec<Vec3>,
    normal: Vec<Vec3>,
    /// World size of a cell, for each volume.
    cell: Vec<f32>,
    /// Volume and chunk coordinates of each chunk of the world.
    places: Vec<(u32, [i32; 3])>,
}

struct Site {
    key: CellKey,
    pos: Vec3,
    normal: Vec3,
}

impl CellSites {
    /// Every cell a vertex of a chunk's full-detail mesh stands in. Border cells
    /// appear in two meshes with the same bits and are one site.
    pub fn of(world: &VoxelWorld) -> Self {
        let mut places = vec![(u32::MAX, [0; 3]); world.chunks.len()];
        let mut cell = Vec::with_capacity(world.volumes.len());
        for (v, geometry) in (0u32..).zip(&world.volumes) {
            cell.push(geometry.chunk_size / CHUNK_EDGE as f32);
            // A hand-made geometry may name an empty axis: the table is then
            // empty too, but the division must not be by zero.
            let [cx, cy, _] = geometry.chunks.map(|c| usize::from(c).max(1));
            for (i, &c) in geometry.table.iter().enumerate() {
                if let Some(slot) = places.get_mut(c as usize).filter(|_| c != NO_CHUNK) {
                    *slot = (v, [i % cx, i / cx % cy, i / (cx * cy)].map(|k| k as i32));
                }
            }
        }
        let mut sites: Vec<Site> = Vec::new();
        for (chunk, &(volume, coord)) in world.chunks.iter().zip(&places) {
            for v in &chunk.lods[0].vertices {
                let global = [0, 1, 2].map(|k| coord[k] * EDGE + i32::from(v.cell[k]));
                sites.push(Site {
                    key: CellKey::from_global(volume, global),
                    pos: v.pos,
                    normal: v.normal,
                });
            }
        }
        sites.sort_by_key(|s| s.key);
        sites.dedup_by_key(|s| s.key);
        Self {
            keys: sites.iter().map(|s| s.key).collect(),
            pos: sites.iter().map(|s| s.pos).collect(),
            normal: sites.iter().map(|s| s.normal).collect(),
            cell,
            places,
        }
    }

    pub fn len(&self) -> usize {
        self.keys.len()
    }

    pub fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }

    pub(super) fn key(&self, i: usize) -> CellKey {
        self.keys[i]
    }

    pub(super) fn pos(&self, i: usize) -> Vec3 {
        self.pos[i]
    }

    pub(super) fn normal(&self, i: usize) -> Vec3 {
        self.normal[i]
    }

    /// World size of the cells of site `i`'s volume.
    pub(super) fn cell_size(&self, i: usize) -> f32 {
        self.cell
            .get(self.keys[i].volume as usize)
            .copied()
            .unwrap_or(1.0)
    }

    /// The site for a vertex in cell `cell` (chunk-local, `-2..=16`) of chunk
    /// `chunk` of the world.
    pub fn lookup(&self, chunk: usize, cell: [i8; 3]) -> Option<usize> {
        let &(volume, coord) = self.places.get(chunk)?;
        if volume == u32::MAX {
            return None;
        }
        let global = [0, 1, 2].map(|k| coord[k] * EDGE + i32::from(cell[k]));
        self.keys
            .binary_search(&CellKey::from_global(volume, global))
            .ok()
    }

    /// The site `delta` cells from site `i`, if there is one.
    pub(super) fn neighbour(&self, i: usize, delta: [i32; 3]) -> Option<usize> {
        let key = self.keys.get(i)?;
        let g = key.global();
        let at = [0, 1, 2].map(|k| g[k].checked_add(delta[k]));
        let [Some(x), Some(y), Some(z)] = at else {
            return None;
        };
        self.keys
            .binary_search(&CellKey::from_global(key.volume, [x, y, z]))
            .ok()
    }
}

/// A hash of the shape of the surface: where every triangle of every chunk is,
/// and nothing about what it is made of. Painting a volume changes only the
/// materials, which light does not read, so it keeps this key and the bake.
pub fn shape_key(world: &VoxelWorld) -> u64 {
    let mut h = Fnv(FNV_BASIS);
    h.u32(world.volumes.len() as u32);
    for v in &world.volumes {
        h.u32(u32::from(v.chunks[0]));
        h.u32(u32::from(v.chunks[1]));
        h.u32(u32::from(v.chunks[2]));
        h.f32(v.chunk_size);
        h.vec3(v.origin);
        for &c in &v.table {
            h.u32(c);
        }
    }
    for chunk in &world.chunks {
        let mesh = &chunk.lods[0];
        h.u32(mesh.vertices.len() as u32);
        for v in &mesh.vertices {
            h.vec3(v.pos);
            h.vec3(v.normal);
            h.bytes(&v.cell.map(|c| c.to_le_bytes()[0]));
        }
        h.u32(mesh.triangles.len() as u32);
        for t in &mesh.triangles {
            for i in t {
                h.u32(u32::from(*i));
            }
        }
    }
    h.0
}
