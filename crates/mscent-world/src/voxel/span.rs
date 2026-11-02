//! Walking the samples of a span of a volume: the shared loop under every edit.
//! It copies only the chunks where a sample really changes.

use std::sync::Arc;

use mscent_math::Vec3d;

use super::chunk::{Chunk, sample_index};
use super::context::{BaseChunk, Context};
use super::volume::{Lattice, Volume};
use super::{CHUNK_EDGE, MAX_SAMPLE, edited};

/// The six samples beside one.
pub(super) const SIDES: [[i32; 3]; 6] = [
    [-1, 0, 0],
    [1, 0, 0],
    [0, -1, 0],
    [0, 1, 0],
    [0, 0, -1],
    [0, 0, 1],
];
/// Samples averaged when smoothing: one and its six neighbours.
const NEIGHBOURS: i32 = 7;

/// Samples an edit may touch, per axis, as `first..end` in the volume.
#[derive(Clone, Copy)]
pub(super) struct Span {
    pub first: [i32; 3],
    pub end: [i32; 3],
}

impl Span {
    pub fn around(lattice: &Lattice, lo: Vec3d, hi: Vec3d, band_cells: f64) -> Option<Self> {
        let cell = lattice.cell.units();
        let samples = lattice.samples();
        let (lo, hi) = ([lo.x, lo.y, lo.z], [hi.x, hi.y, hi.z]);
        let at = |axis: usize, v: f64| v / cell - f64::from(lattice.origin[axis]);
        let first = [0, 1, 2].map(|a| ((at(a, lo[a]) - band_cells).floor() as i32).max(0));
        let end = [0, 1, 2].map(|a| {
            ((at(a, hi[a]) + band_cells).ceil() as i32)
                .saturating_add(1)
                .min(samples[a])
        });
        (0..3)
            .all(|a| first[a] < end[a])
            .then_some(Self { first, end })
    }
}

/// The changed samples, grown as an edit writes.
struct Changed(Option<([i32; 3], [i32; 3])>);

impl Changed {
    fn add(&mut self, g: [i32; 3]) {
        let (lo, hi) = self.0.get_or_insert((g, g));
        for a in 0..3 {
            lo[a] = lo[a].min(g[a]);
            hi[a] = hi[a].max(g[a]);
        }
    }

    /// Every chunk whose mesh reads a changed sample: its own and any that
    /// holds it as padding.
    fn chunks(&self, lattice: &Lattice) -> Vec<[u16; 3]> {
        let Some((lo, hi)) = self.0 else {
            return Vec::new();
        };
        let edge = CHUNK_EDGE as i32;
        let range = |a: usize| {
            let last = i32::from(lattice.chunks[a]) - 1;
            let first = (lo[a] - edge).div_euclid(edge).clamp(0, last);
            first..=(hi[a] + 1).div_euclid(edge).clamp(0, last)
        };
        let mut out = Vec::new();
        for z in range(2) {
            for y in range(1) {
                for x in range(0) {
                    out.push([x as u16, y as u16, z as u16]);
                }
            }
        }
        out
    }
}

/// Runs `edit` over `span`, copying only chunks where a sample really changes.
/// `edit` takes a sample's place, world position, `edited` value and material,
/// and gives its new value and material. Returns the chunks to remesh.
pub(super) fn edit_samples(
    volume: &mut Volume,
    ctx: &mut Context,
    span: Span,
    edit: &mut impl FnMut([i32; 3], Vec3d, i8, u8) -> (i8, u8),
) -> Vec<[u16; 3]> {
    let lattice = volume.lattice;
    let edge = CHUNK_EDGE as i32;
    let mut changed = Changed(None);
    let table = Arc::make_mut(&mut volume.table);
    let first = span.first.map(|g| g.div_euclid(edge));
    let last = span.end.map(|g| (g - 1).div_euclid(edge));
    for cz in first[2]..=last[2] {
        for cy in first[1]..=last[1] {
            for cx in first[0]..=last[0] {
                let coord = [cx, cy, cz];
                let Some(index) = lattice.chunk_index(coord) else {
                    continue;
                };
                ctx.ensure_base(index, coord);
                if let (Some(base), Some(chunk)) = (ctx.cached_base(index), table.get_mut(index)) {
                    edit_chunk(&lattice, chunk, base, coord, span, edit, &mut changed);
                }
            }
        }
    }
    changed.chunks(&lattice)
}

/// The part of [`edit_samples`] inside one chunk.
fn edit_chunk(
    lattice: &Lattice,
    chunk: &mut Chunk,
    base: &BaseChunk,
    coord: [i32; 3],
    span: Span,
    edit: &mut impl FnMut([i32; 3], Vec3d, i8, u8) -> (i8, u8),
    changed: &mut Changed,
) {
    let edge = CHUNK_EDGE as i32;
    let origin = coord.map(|c| c * edge);
    let clip = |a: usize| (span.first[a] - origin[a]).max(0)..(span.end[a] - origin[a]).min(edge);
    let mut wrote = false;
    for z in clip(2) {
        for y in clip(1) {
            for x in clip(0) {
                let i = sample_index(x as usize, y as usize, z as usize);
                let b = base.get(i);
                let (delta, material) = chunk.sample(i);
                let old = edited(b, delta);
                let g = [origin[0] + x, origin[1] + y, origin[2] + z];
                let (value, mat) = edit(g, lattice.position(g), old, material);
                if value == old && mat == material {
                    continue;
                }
                let data = chunk.data_mut();
                data.delta[i] = (i16::from(value) - i16::from(b)) as i8;
                data.material[i] = mat;
                changed.add(g);
                wrote = true;
            }
        }
    }
    if wrote {
        chunk.normalize();
    }
}

pub(super) fn collect_edited(
    volume: &Volume,
    ctx: &mut Context,
    span: Span,
    out: &mut Vec<(i8, u8)>,
) {
    let edge = CHUNK_EDGE as i32;
    let lattice = volume.lattice;
    for z in span.first[2]..span.end[2] {
        for y in span.first[1]..span.end[1] {
            for x in span.first[0]..span.end[0] {
                let coord = [x, y, z].map(|g: i32| g.div_euclid(edge));
                let local = [x, y, z].map(|g: i32| g.rem_euclid(edge) as usize);
                let i = sample_index(local[0], local[1], local[2]);
                let value = lattice.chunk_index(coord).and_then(|index| {
                    ctx.ensure_base(index, coord);
                    let base = ctx.cached_base(index)?.get(i);
                    let (delta, material) = volume.table.get(index)?.sample(i);
                    Some((edited(base, delta), material))
                });
                out.push(value.unwrap_or((-MAX_SAMPLE, 0)));
            }
        }
    }
}

/// `edited` values and materials of a span, for reading neighbours.
pub(super) struct Reader<'a> {
    pub span: Span,
    pub values: &'a [(i8, u8)],
}

impl Reader<'_> {
    pub fn get(&self, g: [i32; 3]) -> (i8, u8) {
        let at = [0, 1, 2].map(|a| {
            (g[a].clamp(self.span.first[a], self.span.end[a] - 1) - self.span.first[a]) as usize
        });
        let size = [0, 1, 2].map(|a| (self.span.end[a] - self.span.first[a]) as usize);
        let i = at[0] + at[1] * size[0] + at[2] * size[0] * size[1];
        self.values.get(i).copied().unwrap_or((-MAX_SAMPLE, 0))
    }

    pub fn beside(&self, g: [i32; 3], side: [i32; 3]) -> (i8, u8) {
        self.get([g[0] + side[0], g[1] + side[1], g[2] + side[2]])
    }

    pub fn average(&self, g: [i32; 3]) -> i32 {
        let sides = SIDES.iter().map(|&s| i32::from(self.beside(g, s).0));
        let sum = i32::from(self.get(g).0) + sides.sum::<i32>();
        (sum + NEIGHBOURS / 2).div_euclid(NEIGHBOURS)
    }

    /// The material of the first solid sample beside `g`.
    pub fn solid_material(&self, g: [i32; 3]) -> Option<u8> {
        SIDES
            .iter()
            .map(|&s| self.beside(g, s))
            .find(|&(value, _)| value >= 0)
            .map(|(_, material)| material)
    }
}
