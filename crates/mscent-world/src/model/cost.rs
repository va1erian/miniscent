//! What a build will take, worked out from the blueprint before any of it is done.
//! A small file must not be able to stall a level's load, as with a recipe. Costs
//! depend only on the blueprint, never a clock, so a file gets the same answer on
//! every machine. The figures are `model_bench`'s, rounded up.

use mscent_math::Vec3d;

use super::{Blueprint, Part, Piece, Soft};
use crate::brush::Op;
use crate::voxel::{CHUNK_EDGE, CellSize, Volume, VolumeError};

/// Most estimated nanoseconds one model may take to build.
pub const MAX_MODEL_NS: u64 = 250_000_000;

/// Meshing a chunk at three levels of detail, on its worst surface.
const CHUNK_NS: u64 = 450_000;
/// Editing one sample of a dab, on the dearest operation (smoothing).
const SAMPLE_NS: u64 = 100;
/// One solid that goes straight to faces.
const SOLID_NS: u64 = 10_000;
/// A part's CSG, per square of the brushes in it.
const CSG_SQUARED_NS: u64 = 90_000;
/// Empty cells around what a part adds. A volume's ring is air, and a surface
/// needs samples beyond it.
const MARGIN_CELLS: f64 = 3.0;

/// A voxel part's box, in cells of its lattice.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct PartBox {
    pub cell: CellSize,
    pub origin: [i32; 3],
    pub chunks: [u16; 3],
}

impl PartBox {
    pub(super) fn chunk_count(&self) -> usize {
        self.chunks.iter().map(|&c| usize::from(c)).product()
    }

    /// An empty add volume of this box.
    pub(super) fn volume(&self) -> Result<Volume, VolumeError> {
        Volume::new(Op::Add, VOLUME_ORDER, self.cell, self.origin, self.chunks)
    }

    fn samples_within(&self, lo: Vec3d, hi: Vec3d) -> u64 {
        let cell = self.cell.units();
        let side = |axis: usize, lo: f64, hi: f64| {
            let start = f64::from(self.origin[axis]);
            let stop = start + f64::from(self.chunks[axis]) * CHUNK_EDGE as f64;
            let width = (hi / cell).min(stop) - (lo / cell).max(start);
            width.max(0.0) as u64 + 1
        };
        side(0, lo.x, hi.x)
            .saturating_mul(side(1, lo.y, hi.y))
            .saturating_mul(side(2, lo.z, hi.z))
    }
}

/// Order of a part's volume in its CSG: after its solids, before its cuts, so that
/// the cuts reach the voxels as they do any add volume.
pub(super) const VOLUME_ORDER: u32 = 1;

/// The box around all a part adds, or `None` if it adds no voxels. Chunks past the
/// most a volume holds are clamped to one too many, so the volume refuses them.
pub(super) fn part_box(part: &Part) -> Option<PartBox> {
    let mut bounds: Option<(Vec3d, Vec3d)> = None;
    for piece in &part.pieces {
        if let Piece::Add { shape, .. } = piece
            && let Some(prim) = shape.prim()
        {
            let (lo, hi) = prim.bounds();
            bounds = Some(match bounds {
                Some((a, b)) => (lowest(a, lo), highest(b, hi)),
                None => (lo, hi),
            });
        }
    }
    let (lo, hi) = bounds?;
    let cell = part.cell.units();
    let first = |v: f64| (v / cell - MARGIN_CELLS).floor();
    let last = |v: f64| (v / cell + MARGIN_CELLS).ceil();
    let long = |lo: f64, hi: f64| {
        let chunks = ((last(hi) - first(lo)) / CHUNK_EDGE as f64).ceil();
        chunks.clamp(1.0, f64::from(u16::MAX)) as u16
    };
    Some(PartBox {
        cell: part.cell,
        origin: [first(lo.x) as i32, first(lo.y) as i32, first(lo.z) as i32],
        chunks: [long(lo.x, hi.x), long(lo.y, hi.y), long(lo.z, hi.z)],
    })
}

fn lowest(a: Vec3d, b: Vec3d) -> Vec3d {
    Vec3d::new(a.x.min(b.x), a.y.min(b.y), a.z.min(b.z))
}

fn highest(a: Vec3d, b: Vec3d) -> Vec3d {
    Vec3d::new(a.x.max(b.x), a.y.max(b.y), a.z.max(b.z))
}

fn soft_of(piece: &Piece) -> Option<&Soft> {
    match piece {
        Piece::Add { shape, .. }
        | Piece::Carve { shape }
        | Piece::Smooth { shape }
        | Piece::Paint { shape, .. } => Some(shape),
        Piece::Solid { .. } | Piece::Cut { .. } => None,
    }
}

fn voxel_ns(part: &Part) -> u64 {
    let Some(b) = part_box(part) else {
        return 0;
    };
    let mesh = (b.chunk_count() as u64).saturating_mul(CHUNK_NS);
    let dabs = part
        .pieces
        .iter()
        .filter_map(soft_of)
        .filter_map(Soft::prim)
        .fold(0u64, |sum, prim| {
            let (lo, hi) = prim.bounds();
            sum.saturating_add(b.samples_within(lo, hi).saturating_mul(SAMPLE_NS))
        });
    mesh.saturating_add(dabs)
}

fn solid_ns(part: &Part) -> u64 {
    let of = |f: fn(&Piece) -> bool| part.pieces.iter().filter(|p| f(p)).count() as u64;
    let solids = of(|p| matches!(p, Piece::Solid { .. }));
    let cuts = of(|p| matches!(p, Piece::Cut { .. }));
    if cuts == 0 {
        return solids.saturating_mul(SOLID_NS);
    }
    let brushes = solids + cuts;
    brushes
        .saturating_mul(brushes)
        .saturating_mul(CSG_SQUARED_NS)
}

/// Estimated nanoseconds to build a model on the dev machine.
pub fn estimated_ns(b: &Blueprint) -> u64 {
    b.parts.iter().fold(0u64, |sum, part| {
        sum.saturating_add(voxel_ns(part))
            .saturating_add(solid_ns(part))
    })
}
