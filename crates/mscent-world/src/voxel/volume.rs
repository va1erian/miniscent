//! A box of the world whose solidity a voxel field decides.

use std::sync::Arc;

use mscent_math::Vec3d;

use super::chunk::Chunk;
use super::{CHUNK_EDGE, MAX_CHUNKS_PER_AXIS};
use crate::brush::{Brush, BrushError, Op};
use crate::materials::NO_DRAW;
use crate::polytope::MAX_COORD;

/// Edge length of a cell. Each is a multiple of the brush snap grid, so a
/// volume's box is always a valid snapped brush.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CellSize {
    Eighth,
    Quarter,
    Half,
    One,
}

impl CellSize {
    /// In world units.
    pub const fn units(self) -> f64 {
        match self {
            CellSize::Eighth => 0.125,
            CellSize::Quarter => 0.25,
            CellSize::Half => 0.5,
            CellSize::One => 1.0,
        }
    }

    /// Its number in the level file.
    pub(super) const fn code(self) -> u8 {
        match self {
            CellSize::Eighth => 0,
            CellSize::Quarter => 1,
            CellSize::Half => 2,
            CellSize::One => 3,
        }
    }

    pub(super) const fn from_code(code: u8) -> Option<Self> {
        match code {
            0 => Some(CellSize::Eighth),
            1 => Some(CellSize::Quarter),
            2 => Some(CellSize::Half),
            3 => Some(CellSize::One),
            _ => None,
        }
    }
}

/// Why a volume was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VolumeError {
    /// A side has no chunks, or more than [`MAX_CHUNKS_PER_AXIS`].
    BadSize,
    /// The box reaches past the range brushes may use.
    OutOfRange,
}

/// Where a volume's lattice lies. Sample `g` of an axis is at
/// `(origin + g) * cell`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Lattice {
    pub op: Op,
    pub order: u32,
    pub cell: CellSize,
    pub origin: [i32; 3],
    pub chunks: [u16; 3],
}

impl Lattice {
    /// Samples along each axis.
    pub fn samples(&self) -> [i32; 3] {
        self.chunks.map(|c| i32::from(c) * CHUNK_EDGE as i32)
    }

    /// World position of sample `g`, which may lie in the ring outside the box.
    pub fn position(&self, g: [i32; 3]) -> Vec3d {
        let cell = self.cell.units();
        let at = |axis: usize| (f64::from(self.origin[axis]) + f64::from(g[axis])) * cell;
        Vec3d::new(at(0), at(1), at(2))
    }

    /// The chunk at `coord`, or `None` outside the volume.
    pub fn chunk_index(&self, coord: [i32; 3]) -> Option<usize> {
        let mut index = 0usize;
        let mut stride = 1usize;
        for (&at, &chunks) in coord.iter().zip(&self.chunks) {
            let count = usize::from(chunks);
            let c = usize::try_from(at).ok().filter(|&c| c < count)?;
            index += c * stride;
            stride *= count;
        }
        Some(index)
    }

    pub fn chunk_count(&self) -> usize {
        self.chunks.iter().map(|&c| usize::from(c)).product()
    }
}

/// A voxel volume. Cloning shares all chunks, and an edit copies only those it
/// changes, so a clone serves as an undo step or a stroke snapshot.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Volume {
    pub(super) lattice: Lattice,
    pub(super) table: Arc<Vec<Chunk>>,
}

impl Volume {
    /// An untouched volume: an [`Op::Add`] one holds only air, an
    /// [`Op::Subtract`] one looks just like the brushes it covers. `origin`
    /// is in cells, `chunks` in chunks per axis.
    pub fn new(
        op: Op,
        order: u32,
        cell: CellSize,
        origin: [i32; 3],
        chunks: [u16; 3],
    ) -> Result<Self, VolumeError> {
        if chunks.iter().any(|&c| c == 0 || c > MAX_CHUNKS_PER_AXIS) {
            return Err(VolumeError::BadSize);
        }
        let lattice = Lattice {
            op,
            order,
            cell,
            origin,
            chunks,
        };
        let (lo, hi) = (
            lattice.position([0; 3]),
            lattice.position(lattice.samples()),
        );
        let reach = [lo.x, lo.y, lo.z, hi.x, hi.y, hi.z];
        if reach.iter().any(|v| v.abs() > MAX_COORD) {
            return Err(VolumeError::OutOfRange);
        }
        let table = Arc::new(vec![Chunk::EMPTY; lattice.chunk_count()]);
        Ok(Self { lattice, table })
    }

    pub fn op(&self) -> Op {
        self.lattice.op
    }

    pub fn order(&self) -> u32 {
        self.lattice.order
    }

    /// The same volume at another place in the CSG order.
    #[must_use]
    pub fn with_order(mut self, order: u32) -> Self {
        self.lattice.order = order;
        self
    }

    pub fn cell(&self) -> CellSize {
        self.lattice.cell
    }

    /// Chunks per axis.
    pub fn chunks(&self) -> [u16; 3] {
        self.lattice.chunks
    }

    /// Corners of the box.
    pub fn bounds(&self) -> (Vec3d, Vec3d) {
        (
            self.lattice.position([0; 3]),
            self.lattice.position(self.lattice.samples()),
        )
    }

    /// The chunk at `coord`, if the volume has one there.
    pub fn chunk(&self, coord: [u16; 3]) -> Option<&Chunk> {
        let index = self.lattice.chunk_index(coord.map(i32::from))?;
        self.table.get(index)
    }

    /// Whether the two boxes share space. Touching does not count.
    pub fn overlaps(&self, other: &Volume) -> bool {
        let ((a0, a1), (b0, b1)) = (self.bounds(), other.bounds());
        a0.x < b1.x && b0.x < a1.x && a0.y < b1.y && b0.y < a1.y && a0.z < b1.z && b0.z < a1.z
    }

    /// The brush that clears this volume's box from the BSP: a subtract brush
    /// at the volume's place in the order, whose faces are never drawn. An
    /// [`Op::Add`] volume has none.
    pub fn shell(&self) -> Option<Result<Brush, BrushError>> {
        let (lo, hi) = self.bounds();
        (self.op() == Op::Subtract).then(|| {
            Brush::cuboid(lo, hi, Op::Subtract, self.order()).map(|b| b.with_material(NO_DRAW))
        })
    }
}
