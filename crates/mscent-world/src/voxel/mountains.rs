//! A mountain range as an [`Op::Add`] volume: a ridged-noise heightfield, solid
//! below its surface, for a distant view (design in `docs/viewports.md`).
//!
//! The generator is pure. The same [`Range`] always gives the same volume, bit for
//! bit: it uses seeded integer-hash noise and only IEEE arithmetic (no `powf`,
//! `sin`, or anything a platform's libm could round differently).
//!
//! The surface is `ground + peak * shape` above the box's floor. `shape` blends two
//! forms by a broad swell of noise: low foothills, and massifs that rise to half
//! the peak with ridged crests (sharp where the noise crosses zero, each octave
//! weighted by the one before so crests carry the detail and valleys stay smooth)
//! riding on them, squared so that few reach the top. An envelope takes it to nothing
//! at the box's edges, so no cliff shows there. A [`Clearing`] does the same along a
//! road through the range, so a whole room's ground can be one range with a flat way
//! through it. It is divided by its own highest point, so the tallest peak is
//! exactly `peak`.
//!
//! The field handed to [`fill_with`] is the height above the surface over the
//! slope's length, which keeps its value near a true distance so the mesher places
//! steep flanks where the formula says. Heights are computed once per column of the
//! lattice, not once per sample, and so are the [`Layers`]' materials: only the
//! samples near the surface take them, as only those show, and the rest keep the
//! range's own material so that buried and empty chunks stay uniform.

use mscent_math::Vec3d;

use super::layers::Layers;
use super::noise::noise3;
use super::volume::{CellSize, Volume, VolumeError};
use super::wire::MAX_DETAIL_CHUNKS;
use super::{CHUNK_EDGE, Chunk, Context, MAX_CHUNKS_PER_AXIS, MAX_SAMPLE, fill_with, quantize};
use crate::brush::Op;
use crate::polytope::MAX_COORD;

/// Solid under the lowest ground, in cells, so the range has a floor to stand on.
const GROUND_CELLS: f64 = 2.0;
/// Octaves of ridged noise.
const OCTAVES: u32 = 5;
/// How far in from the box's edge, as a share of its width, the range reaches
/// full height, unless the range says how far in world units.
const FALLOFF: f64 = 0.3;
/// How far past a clearing's edge, as a share of its radius, the range is back
/// to its own height.
const CLEAR_EASE: f64 = 0.5;
/// Wavelengths across the longer side, of the first octave.
const RIDGES_ACROSS: f64 = 2.5;
/// Height of the lowlands, and the relief the crests add to them, as shares of
/// the peak.
const LOWLAND: f64 = 0.08;
const FOOTHILL_RELIEF: f64 = 0.14;
/// The ridged sum between which a crest goes from none to all.
const CREST_LOW: f64 = 0.35;
const CREST_HIGH: f64 = 0.8;
/// Height a massif rises to before its crests, as a share of the peak.
const MASSIF_BASE: f64 = 0.5;
/// Swells per first-octave wavelength, and the broad noise values between which a
/// swell goes from nothing to all of a massif.
const SWELL_FREQ: f64 = 0.5;
const SWELL_LOW: f64 = 0.35;
const SWELL_HIGH: f64 = 0.62;

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
    /// Voxel material of the rock, where no layer takes the ground.
    pub material: u8,
    pub order: u32,
    /// A road left flat at the ground, if any.
    pub clear: Option<Clearing>,
    /// How far in from the box's sides the range reaches full height, in world
    /// units; `None` is `FALLOFF` of each side. A range that is a room's whole
    /// ground wants a narrow edge, or its walls take most of it.
    pub edge: Option<f64>,
    /// Materials by height and slope; [`Layers::NONE`] leaves it all `material`.
    pub layers: Layers,
}

/// A road through a range, flat at its ground: every column within `radius` of the
/// segment from `a` to `b` (in plan, world units), easing back to the range's own
/// height over `CLEAR_EASE` of that again.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Clearing {
    pub a: [f64; 2],
    pub b: [f64; 2],
    pub radius: f64,
}

impl Clearing {
    fn is_valid(&self) -> bool {
        let ends = [self.a, self.b].concat();
        let in_range = ends.iter().all(|v| v.is_finite() && v.abs() <= MAX_COORD);
        in_range && self.radius.is_finite() && self.radius > 0.0 && self.radius <= MAX_COORD
    }

    /// 0 on the road, 1 once past its ease, smooth at both ends.
    fn share(&self, x: f64, y: f64) -> f64 {
        let (ab, ap) = (
            [self.b[0] - self.a[0], self.b[1] - self.a[1]],
            [x - self.a[0], y - self.a[1]],
        );
        let len2 = ab[0] * ab[0] + ab[1] * ab[1];
        let t = if len2 > 0.0 {
            ((ap[0] * ab[0] + ap[1] * ab[1]) / len2).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let (dx, dy) = (ap[0] - ab[0] * t, ap[1] - ab[1] * t);
        let ease = self.radius * CLEAR_EASE;
        let s = (((dx * dx + dy * dy).sqrt() - self.radius) / ease).clamp(0.0, 1.0);
        s * s * (3.0 - 2.0 * s)
    }
}

/// Why a range was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MountainError {
    /// A bound or the peak is not a finite number, or the peak is not above zero.
    NotFinite,
    /// A side of the box has no length, the box lies past the range brushes may
    /// use, or the clearing or the edge is not finite, positive and within it.
    BadBounds,
    /// A layer's bound or the jitter is not a finite number in range.
    BadLayers,
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
            Self::BadLayers => write!(f, "a layer's bound or the jitter is out of range"),
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
    let edge_ok = |e: f64| e.is_finite() && e > 0.0 && e <= MAX_COORD;
    let extras_ok = r.clear.is_none_or(|c| c.is_valid()) && r.edge.is_none_or(edge_ok);
    if corners.iter().any(|v| v.abs() > MAX_COORD) || !extras_ok {
        return Err(MountainError::BadBounds);
    }
    if !r.layers.is_valid() {
        return Err(MountainError::BadLayers);
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
    fill_with(&mut volume, &mut ctx, |p| {
        let (distance, layered) = heights.field(p);
        let shown = (0..MAX_SAMPLE).contains(&quantize(distance, cell));
        (distance, if shown { layered } else { r.material })
    });
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
    /// The material of each column, parallel to `z`.
    material: Vec<u8>,
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
        let falloff = |side: f64| r.edge.map_or(FALLOFF, |e| e / side);
        let (fx, fy) = (falloff(hi.x - lo.x), falloff(hi.y - lo.y));
        let mut raw = Vec::with_capacity(stride * rows);
        for j in 0..rows {
            for i in 0..stride {
                let x = (origin[0] + i as i64 - 1) as f64 * cell;
                let y = (origin[1] + j as i64 - 1) as f64 * cell;
                let (u, v) = ((x - lo.x) / (hi.x - lo.x), (y - lo.y) / (hi.y - lo.y));
                let q = Vec3d::new(x - lo.x, y - lo.y, 0.0) * (1.0 / wave);
                let road = r.clear.map_or(1.0, |c| c.share(x, y));
                raw.push(range_shape(r.seed, q) * envelope(u, fx) * envelope(v, fy) * road);
            }
        }
        // The tallest crest is the peak, however the noise fell.
        let top = raw.iter().copied().fold(0.0, f64::max);
        let scale = if top > 0.0 { r.peak / top } else { 0.0 };
        let ground = lo.z + GROUND_CELLS * cell;
        let z: Vec<f64> = raw.into_iter().map(|h| ground + h * scale).collect();
        let mut heights = Self {
            cell,
            origin,
            stride,
            z,
            material: Vec::new(),
        };
        heights.material = heights.layered(r, ground, lo, wave);
        heights
    }

    /// Each column's material by the range's layers, reading its height above
    /// `ground` as a share of the peak and its slope from the neighbours' heights.
    fn layered(&self, r: &Range, ground: f64, lo: Vec3d, wave: f64) -> Vec<u8> {
        if r.layers.is_empty() {
            return vec![r.material; self.z.len()];
        }
        let mut out = Vec::with_capacity(self.z.len());
        for (at, &h) in self.z.iter().enumerate() {
            let (i, j) = (at % self.stride, at / self.stride);
            let slope = self
                .gradient(i, j)
                .map_or(0.0, |[gx, gy]| (gx * gx + gy * gy).sqrt());
            let x = (self.origin[0] + i as i64 - 1) as f64 * self.cell;
            let y = (self.origin[1] + j as i64 - 1) as f64 * self.cell;
            let q = Vec3d::new(x - lo.x, y - lo.y, 0.0) * (1.0 / wave);
            let share = (h - ground) / r.peak;
            out.push(r.layers.pick(r.seed, q, share, slope, r.material));
        }
        out
    }

    /// Rise over run across column `(i, j)`, if it has neighbours on all sides.
    fn gradient(&self, i: usize, j: usize) -> Option<[f64; 2]> {
        let (w, e) = (self.at(i.checked_sub(1)?, j)?, self.at(i + 1, j)?);
        let (s, n) = (self.at(i, j.checked_sub(1)?)?, self.at(i, j + 1)?);
        Some([(e - w) / (2.0 * self.cell), (n - s) / (2.0 * self.cell)])
    }

    fn at(&self, i: usize, j: usize) -> Option<f64> {
        if i >= self.stride {
            return None;
        }
        let at = j.checked_mul(self.stride)?.checked_add(i)?;
        self.z.get(at).copied()
    }

    /// Height above the surface at `p` over the length of the slope, positive
    /// below it, and the material of its column. Off the table, or on a
    /// non-finite point, it is air.
    fn field(&self, p: Vec3d) -> (f64, u8) {
        let col =
            |v: f64, origin: i64| usize::try_from((v / self.cell).round() as i64 - origin + 1);
        let air = (-self.cell, 0);
        let (Ok(i), Ok(j)) = (col(p.x, self.origin[0]), col(p.y, self.origin[1])) else {
            return air;
        };
        let at = j.checked_mul(self.stride).and_then(|a| a.checked_add(i));
        let (Some(h), Some([gx, gy]), Some(&m)) = (
            self.at(i, j),
            self.gradient(i, j),
            at.and_then(|a| self.material.get(a)),
        ) else {
            return air;
        };
        ((h - p.z) / (1.0 + gx * gx + gy * gy).sqrt(), m)
    }
}

/// 0 at the box's edge to 1 once `falloff` of its width in, smooth at both ends.
fn envelope(t: f64, falloff: f64) -> f64 {
    let edge = t.min(1.0 - t) / falloff;
    let s = edge.clamp(0.0, 1.0);
    s * s * (3.0 - 2.0 * s)
}

/// Ridged multifractal at `q` (in first-octave wavelengths), `0..=1`: crests
/// sharpened, each octave weighted by the ridge of the one before, so crests carry
/// the detail and valleys stay smooth.
fn crest_at(seed: u32, q: Vec3d) -> f64 {
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
    sum / norm
}

/// How much of a massif `q` is on, `0..=1`: two octaves of broad noise passed
/// through a smoothstep, so the swells have wide shoulders and the lowlands between
/// them stay low.
fn swell_at(seed: u32, q: Vec3d) -> f64 {
    let p = Vec3d::new(q.x * SWELL_FREQ, q.y * SWELL_FREQ, 3.1);
    let n = noise3(seed ^ 0x5bd1_e995, p) + 0.5 * noise3(seed ^ 0x2545_f491, p * 2.3);
    let t = ((n / 1.5) * 0.5 + 0.5 - SWELL_LOW) / (SWELL_HIGH - SWELL_LOW);
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// The surface's shape at `q`, `0..=1` before it is scaled to the peak. The
/// lowlands are foothills with a little relief; where a swell rises, the rock
/// climbs to half the peak and crests ride on it, squared so that few of them reach
/// the top: a massif with a few peaks, not a field of spikes.
fn range_shape(seed: u32, q: Vec3d) -> f64 {
    // The ridged sum sits between about 0.3 and 0.85: spread it over 0..1.
    let crest = ((crest_at(seed, q) - CREST_LOW) / (CREST_HIGH - CREST_LOW)).clamp(0.0, 1.0);
    let swell = swell_at(seed, q);
    let foothills = LOWLAND + FOOTHILL_RELIEF * crest;
    let massif = MASSIF_BASE + (1.0 - MASSIF_BASE) * crest * crest;
    foothills + (massif - foothills) * swell
}

#[cfg(test)]
#[path = "mountains_tests.rs"]
mod tests;
