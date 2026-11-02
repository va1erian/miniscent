//! Voxel volumes: a second kind of geometry beside brushes (design in
//! `docs/voxels.md`).
//!
//! A [`Volume`] is a step in the ordered CSG. It stores signed *changes* over
//! the brushes rather than final density, so moving a brush updates untouched
//! rock while sculpting survives. For a sample of a volume at order `k`:
//!
//! ```text
//! base     = brushes with order < k, starting solid     (Op::Subtract)
//!          = air                                        (Op::Add)
//! edited   = clamp(base + delta)
//! resolved = edited cut by the subtract brushes with order > k
//! ```
//!
//! Later add brushes are left out on purpose: the BSP already holds them, and
//! a field that followed them would lay a surface exactly on their faces.
//!
//! A point of the world is solid if the BSP or a resolved field says so. The
//! sign of a sample decides: zero and above is solid. One unit of a sample is
//! 1/32 of a cell, which keeps a plane through the lattice exactly flat.
//!
//! * [`Volume`] (`volume.rs`): sparse copy-on-write chunks. Cloning is O(1).
//! * [`Context`] (`context.rs`): what the brushes contribute to one volume.
//! * [`apply_dab`], [`fill`] (`dab.rs`): edits, returning the chunks to remesh.
//! * [`capture`], [`stamp`] (`clip.rs`): Copy and Stamp.
//! * [`noise3`] (`noise.rs`): seeded noise for roughening and generated fields.
//! * [`Mesher`] (`surface.rs`): surface nets, one [`ChunkMesh`] per chunk.
//! * [`ChunkMesh::clustered`] (`lod.rs`): level of detail without cracks.
//! * [`AirMask`] (`connectivity.rs`): which chunk faces air joins.
//! * [`VoxelWorld`] (`world.rs`): every volume of a level meshed against its BSP.
//! * [`CellIndex`] (`index.rs`) and the queries in `query.rs`: the triangles of a
//!   box, and whether a point is solid, for collision.
//! * `wire.rs`: the level file's section for volumes.

mod block;
mod chunk;
mod clip;
mod connectivity;
mod context;
mod dab;
mod edit;
mod index;
mod lod;
mod noise;
mod prim;
mod query;
mod span;
mod surface;
mod volume;
pub(crate) mod wire;
mod world;

#[cfg(test)]
mod dab_tests;
#[cfg(test)]
mod edit_tests;
#[cfg(test)]
mod query_tests;
#[cfg(test)]
mod resolve_tests;
#[cfg(test)]
mod surface_tests;
#[cfg(test)]
mod test_util;
#[cfg(test)]
mod tools_tests;
#[cfg(test)]
mod wire_tests;
#[cfg(test)]
mod world_tests;

pub use chunk::{Chunk, ChunkData};
pub use clip::{Clip, MAX_CLIP_HALF, capture, stamp};
pub use connectivity::{AirMask, Face};
pub use context::Context;
pub use dab::{DabOp, apply_dab, fill};
pub use edit::{MIN_ISLAND_SAMPLES, first_solid, remove_islands};
pub use index::{CellIndex, INDEX_EDGE};
pub use lod::Lod;
pub use noise::noise3;
pub use prim::Prim;
pub use surface::{ChunkMesh, Mesher, VoxelVertex};
pub use volume::{CellSize, Volume, VolumeError};
pub use wire::{MAX_DETAIL_CHUNKS, MAX_VOLUMES};
pub use world::{NO_CHUNK, RemeshedChunk, VolumeGeometry, VoxelChunk, VoxelWorld};

/// Samples along one edge of a chunk.
pub const CHUNK_EDGE: usize = 16;
/// Samples in a chunk.
pub const CHUNK_SAMPLES: usize = CHUNK_EDGE * CHUNK_EDGE * CHUNK_EDGE;
/// Most chunks along one axis of a volume.
pub const MAX_CHUNKS_PER_AXIS: u16 = 16;
/// Sample units per cell.
pub const UNITS_PER_CELL: f64 = 32.0;
/// Largest magnitude of a `base`, `edited` or `resolved` sample: two cells.
pub const MAX_SAMPLE: i8 = 63;
/// Largest magnitude of a `delta`: enough to override any base.
pub const MAX_DELTA: i8 = 2 * MAX_SAMPLE;

/// A stored material meaning "whatever brush this sample came from". Rock nobody
/// painted looks like the wall it was carved out of.
pub const INHERIT_MATERIAL: u8 = u8::MAX;
/// What a sample shows for a brush material samples cannot name: not in any
/// table, so it draws with the grey fallback.
pub const FALLBACK_MATERIAL: u8 = u8::MAX - 1;

/// World distance `d` as a sample, for a lattice of `cell` world units. The
/// sign always survives, so a surface never moves to the wrong side of a
/// sample by rounding. NaN counts as air.
pub(crate) fn quantize(d: f64, cell: f64) -> i8 {
    if d.is_nan() {
        return -MAX_SAMPLE;
    }
    let limit = f64::from(MAX_SAMPLE);
    let v = (d / cell * UNITS_PER_CELL).round().clamp(-limit, limit) as i8;
    if d < 0.0 { v.min(-1) } else { v }
}

/// `base + delta`, limited to the sample range.
pub(crate) fn edited(base: i8, delta: i8) -> i8 {
    (i16::from(base) + i16::from(delta)).clamp(i16::from(-MAX_SAMPLE), i16::from(MAX_SAMPLE)) as i8
}
