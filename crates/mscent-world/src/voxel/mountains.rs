//! A mountain range as an [`Op::Add`] volume: a ridged-noise heightfield, solid
//! below its surface, for a distant view (design in `docs/viewports.md`).
//!
//! The generator is pure. The same [`Range`] always gives the same volume, bit for
//! bit: it uses seeded integer-hash noise and only IEEE arithmetic (no `powf`,
//! `sin`, or anything a platform's libm could round differently).
//!
//! The surface is `ground + peak * shape` above the box's floor. `shape` is ridged
//! noise (sharp crests where the noise crosses zero, each octave weighted by the
//! one before so crests carry the detail and valleys stay smooth), scaled down by
//! a broad mask so some of the range is foothills, and by an envelope that takes
//! it to nothing at the box's edges, so no cliff shows there. It is divided by its
//! own highest point, so the tallest peak is exactly `peak`.
//!
//! The field handed to [`fill`] is the height above the surface over the slope's
//! length, which keeps its value near a true distance so the mesher places steep
//! flanks where the formula says. Heights are computed once per column of the
//! lattice, not once per sample.

use mscent_math::Vec3d;

use super::noise::noise3;
use super::volume::{CellSize, Volume, VolumeError};
use super::wire::MAX_DETAIL_CHUNKS;
use super::{CHUNK_EDGE, Chunk, Context, MAX_CHUNKS_PER_AXIS, fill};
use crate::brush::Op;
use crate::polytope::MAX_COORD;

/// Solid under the lowest ground, in cells, so the range has a floor to stand on.
const GROUND_CELLS: f64 = 2.0;
/// Octaves of ridged noise.
const OCTAVES: u32 = 5;
/// How far in from the box's edge, as a share of its width, the range reaches
/// full height.
const FALLOFF: f64 = 0.3;
/// Wavelengths across the longer side, of the first octave.
const RIDGES_ACROSS: f64 = 2.5;
/// Lowest the foothill mask goes, as a share of full height.
const FOOTHILL_FLOOR: f64 = 0.3;

/// What to build. Boxes are in world units; the volume starts at `min` rounded
/// down to a cell and grows to a whole number of chunks (16 cells) from there.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Range {
    pub min: Vec3d,
    pub max: Vec3d,
    pub cell: CellSize,
    /// Height of the tallest peak above the ground, in world units.
    pub peak: f64,
    pub seed: u32,
    /// Voxel material of the rock.
    pub material: u8,
    pub order: u32,
}

/// Why a range was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MountainError {
    /// A bound or the peak is not a finite number, or the peak is not above zero.
    NotFinite,
    /// A side of the box has no length, or the box lies past the range brushes may use.
    BadBounds,
    /// A side needs more than [`MAX_CHUNKS_PER_AXIS`] chunks at this cell size.
    TooLarge,
    /// The box is not tall enough for the ground and the peak.
    PeakAboveBox,
    /// The surface would need more detail chunks than a level file may hold.
    TooDetailed,
    Volume(VolumeError),
}

impl std::fmt::Display for MountainError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFinite => write!(f, "a bound or the peak is not a finite number"),
            Self::BadBounds => write!(f, "the box is empty or out of range"),
            Self::TooLarge => write!(
                f,
                "the box needs more than {MAX_CHUNKS_PER_AXIS} chunks on a side at this cell size"
            ),
            Self::PeakAboveBox => write!(f, "the box is too low for the ground and the peak"),
            Self::TooDetailed => write!(f, "the range has too many detail chunks for a level"),
            Self::Volume(e) => write!(f, "volume refused: {e:?}"),
        }
    }
}

/// Chunks per axis for a box of `size`, or the reason it cannot be.
fn chunk_counts(size: [f64; 3], cell: f64) -> Result<[u16; 3], MountainError> {
    let edge = CHUNK_EDGE as f64 * cell;
    let mut out = [0u16; 3];
    for (slot, s) in out.iter_mut().zip(size) {
        let n = (s / edge).ceil();
        if n.is_nan() || n < 1.0 {
            return Err(MountainError::BadBounds);
        }
        if n > f64::from(MAX_CHUNKS_PER_AXIS) {
            return Err(MountainError::TooLarge);
        }
        *slot = n as u16;
    }
    Ok(out)
}

/// Builds the range, or says why not. Never panics, whatever the input.
pub fn mountains(r: &Range) -> Result<Volume, MountainError> {
    let corners = [r.min.x, r.min.y, r.min.z, r.max.x, r.max.y, r.max.z];
    if !(corners.iter().all(|v| v.is_finite()) && r.peak.is_finite() && r.peak > 0.0) {
        return Err(MountainError::NotFinite);
    }
    if corners.iter().any(|v| v.abs() > MAX_COORD) {
        return Err(MountainError::BadBounds);
    }
    let cell = r.cell.units();
    let size = [r.max.x - r.min.x, r.max.y - r.min.y, r.max.z - r.min.z];
    if size[2] < GROUND_CELLS * cell + r.peak {
        return Err(MountainError::PeakAboveBox);
    }
    let chunks = chunk_counts(size, cell)?;
    let origin = [r.min.x, r.min.y, r.min.z].map(|v| (v / cell).floor() as i32);
    let mut volume =
        Volume::new(Op::Add, r.order, r.cell, origin, chunks).map_err(MountainError::Volume)?;
    let heights = Heights::new(&volume, r);
    let mut ctx = Context::new(&volume, &[]);
    fill(&mut volume, &mut ctx, |p| heights.field(p), r.material);
    let detailed = chunks_with_detail(&volume);
    if detailed > MAX_DETAIL_CHUNKS {
        return Err(MountainError::TooDetailed);
    }
    Ok(volume)
}

fn chunks_with_detail(volume: &Volume) -> usize {
    let [nx, ny, nz] = volume.chunks();
    let mut count = 0;
    for z in 0..nz {
        for y in 0..ny {
            for x in 0..nx {
                if matches!(volume.chunk([x, y, z]), Some(Chunk::Detail(_))) {
                    count += 1;
                }
            }
        }
    }
    count
}

/// The surface's height over every column of the lattice, with a ring outside
/// for slopes at the edge.
struct Heights {
    cell: f64,
    origin: [i64; 2],
    /// Columns per row, ring included.
    stride: usize,
    z: Vec<f64>,
}

impl Heights {
    fn new(volume: &Volume, r: &Range) -> Self {
        let (lo, hi) = volume.bounds();
        let cell = r.cell.units();
        let lattice = volume.chunks().map(|c| usize::from(c) * CHUNK_EDGE);
        let origin = [(lo.x / cell).round() as i64, (lo.y / cell).round() as i64];
        let stride = lattice[0] + 2;
        let rows = lattice[1] + 2;
        let wave = (hi.x - lo.x).max(hi.y - lo.y) / RIDGES_ACROSS;
        let mut raw = Vec::with_capacity(stride * rows);
        for j in 0..rows {
            for i in 0..stride {
                let x = (origin[0] + i as i64 - 1) as f64 * cell;
                let y = (origin[1] + j as i64 - 1) as f64 * cell;
                let (u, v) = ((x - lo.x) / (hi.x - lo.x), (y - lo.y) / (hi.y - lo.y));
                let q = Vec3d::new(x - lo.x, y - lo.y, 0.0) * (1.0 / wave);
                raw.push(range_shape(r.seed, q) * envelope(u) * envelope(v));
            }
        }
        // The tallest crest is the peak, however the noise fell.
        let top = raw.iter().copied().fold(0.0, f64::max);
        let scale = if top > 0.0 { r.peak / top } else { 0.0 };
        let ground = lo.z + GROUND_CELLS * cell;
        let z = raw.into_iter().map(|h| ground + h * scale).collect();
        Self {
            cell,
            origin,
            stride,
            z,
        }
    }

    fn at(&self, i: usize, j: usize) -> Option<f64> {
        self.z.get(j * self.stride + i).copied()
    }

    /// Height above the surface at `p` over the length of the slope, positive
    /// below it. Off the table, or on a non-finite point, it is air.
    fn field(&self, p: Vec3d) -> f64 {
        let col =
            |v: f64, origin: i64| usize::try_from((v / self.cell).round() as i64 - origin + 1);
        let (Ok(i), Ok(j)) = (col(p.x, self.origin[0]), col(p.y, self.origin[1])) else {
            return -self.cell;
        };
        let (Some(h), Some(w), Some(e), Some(s), Some(n)) = (
            self.at(i, j),
            self.at(i.wrapping_sub(1), j),
            self.at(i + 1, j),
            self.at(i, j.wrapping_sub(1)),
            self.at(i, j + 1),
        ) else {
            return -self.cell;
        };
        let (gx, gy) = ((e - w) / (2.0 * self.cell), (n - s) / (2.0 * self.cell));
        (h - p.z) / (1.0 + gx * gx + gy * gy).sqrt()
    }
}

/// 0 at the box's edge to 1 once `FALLOFF` of its width in, smooth at both ends.
fn envelope(t: f64) -> f64 {
    let edge = t.min(1.0 - t) / FALLOFF;
    let s = edge.clamp(0.0, 1.0);
    s * s * (3.0 - 2.0 * s)
}

/// Ridged multifractal at `q` (in first-octave wavelengths), roughly `0..=1`:
/// crests sharpened, cut down by a broad mask so there are foothills.
fn range_shape(seed: u32, q: Vec3d) -> f64 {
    let (mut sum, mut norm) = (0.0, 0.0);
    let (mut amp, mut freq, mut weight) = (1.0, 1.0, 1.0);
    for o in 0..OCTAVES {
        let at = Vec3d::new(q.x * freq, q.y * freq, 0.37);
        let n = noise3(seed.wrapping_add(o.wrapping_mul(0x9E37)), at);
        let ridge = 1.0 - n.abs();
        let ridge = ridge * ridge;
        sum += ridge * weight * amp;
        norm += amp;
        weight = (ridge * 2.0).min(1.0);
        amp *= 0.5;
        freq *= 2.07;
    }
    let crest = sum / norm;
    let broad = noise3(seed ^ 0x5bd1_e995, Vec3d::new(q.x * 0.6, q.y * 0.6, 3.1)) * 0.5 + 0.5;
    let mask = FOOTHILL_FLOOR + (1.0 - FOOTHILL_FLOOR) * broad * broad;
    // Cubed: low ridges flatten into foothills while crests stay.
    crest * crest * crest * mask
}

#[cfg(test)]
#[path = "mountains_tests.rs"]
mod tests;
