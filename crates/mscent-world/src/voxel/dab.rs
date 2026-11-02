//! Edits to a volume. An edit reads and writes `edited` values and stores them
//! as changes over the base, so the brushes below stay free to move.

use std::sync::Arc;

use mscent_math::Vec3d;

use super::chunk::{Chunk, sample_index};
use super::context::{BaseChunk, Context};
use super::prim::Prim;
use super::volume::{Lattice, Volume};
use super::{CHUNK_EDGE, MAX_SAMPLE, edited, quantize};

/// Cells past a shape where an add or carve can still change a sample: the
/// sample range is two cells deep.
const BAND_CELLS: f64 = 2.0;
/// Samples averaged when smoothing: one and its six neighbours.
const NEIGHBOURS: i32 = 7;
/// How fast a smoothing dab reaches full strength going in from its edge,
/// against the diagonal of its bounds.
const CORE_SHARPNESS: f64 = 4.0;

/// What a dab does inside its shape.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DabOp {
    /// Fills with solid of a palette material.
    Add { material: u8 },
    /// Removes solid.
    Carve,
    /// Moves samples toward the average of their neighbours, fully at the
    /// shape's core and not at all at its edge.
    Smooth,
    /// Changes the material only.
    Paint { material: u8 },
}

/// Samples an edit may touch, per axis, as `first..end` in the volume.
#[derive(Clone, Copy)]
struct Span {
    first: [i32; 3],
    end: [i32; 3],
}

impl Span {
    fn around(lattice: &Lattice, lo: Vec3d, hi: Vec3d, band_cells: f64) -> Option<Self> {
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
fn edit_samples(
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

/// Applies one dab. A context built for another volume, or a shape that misses
/// the box, changes nothing. Returns the chunks to remesh.
pub fn apply_dab(volume: &mut Volume, ctx: &mut Context, prim: &Prim, op: DabOp) -> Vec<[u16; 3]> {
    if ctx.lattice != volume.lattice {
        return Vec::new();
    }
    let lattice = volume.lattice;
    let cell = lattice.cell.units();
    let (lo, hi) = prim.bounds();
    let band = match op {
        DabOp::Add { .. } | DabOp::Carve => BAND_CELLS,
        DabOp::Smooth | DabOp::Paint { .. } => 0.0,
    };
    let Some(span) = Span::around(&lattice, lo, hi, band) else {
        return Vec::new();
    };
    match op {
        DabOp::Add { material } => edit_samples(volume, ctx, span, &mut |_, p, value, old| {
            let shape = quantize(-prim.distance(p), cell);
            let m = if shape >= 0 { material } else { old };
            (value.max(shape), m)
        }),
        DabOp::Carve => edit_samples(volume, ctx, span, &mut |_, p, value, old| {
            (value.min(quantize(prim.distance(p), cell)), old)
        }),
        DabOp::Paint { material } => edit_samples(volume, ctx, span, &mut |_, p, value, old| {
            (
                value,
                if prim.distance(p) <= 0.0 {
                    material
                } else {
                    old
                },
            )
        }),
        DabOp::Smooth => smooth(volume, ctx, prim, span),
    }
}

/// Smoothing reads all values first, so the result does not depend on the
/// order samples are visited in.
fn smooth(volume: &mut Volume, ctx: &mut Context, prim: &Prim, span: Span) -> Vec<[u16; 3]> {
    let (lo, hi) = prim.bounds();
    let size = (hi - lo).length().max(volume.lattice.cell.units());
    let mut values = Vec::new();
    collect_edited(volume, ctx, span, &mut values);
    let reader = Reader {
        span,
        values: &values,
    };
    edit_samples(volume, ctx, span, &mut |g, p, value, old| {
        let weight = (-prim.distance(p) / size * CORE_SHARPNESS).clamp(0.0, 1.0);
        let target = f64::from(reader.average(g));
        let moved = f64::from(value) + (target - f64::from(value)) * weight;
        let limit = f64::from(MAX_SAMPLE);
        (moved.round().clamp(-limit, limit) as i8, old)
    })
}

fn collect_edited(volume: &Volume, ctx: &mut Context, span: Span, out: &mut Vec<i8>) {
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
                    Some(edited(base, volume.table.get(index)?.sample(i).0))
                });
                out.push(value.unwrap_or(-MAX_SAMPLE));
            }
        }
    }
}

/// `edited` values of a span, for reading neighbours.
struct Reader<'a> {
    span: Span,
    values: &'a [i8],
}

impl Reader<'_> {
    fn get(&self, g: [i32; 3]) -> i32 {
        let at = [0, 1, 2].map(|a| {
            (g[a].clamp(self.span.first[a], self.span.end[a] - 1) - self.span.first[a]) as usize
        });
        let size = [0, 1, 2].map(|a| (self.span.end[a] - self.span.first[a]) as usize);
        let i = at[0] + at[1] * size[0] + at[2] * size[0] * size[1];
        self.values.get(i).map_or(0, |&v| i32::from(v))
    }

    fn average(&self, g: [i32; 3]) -> i32 {
        let [x, y, z] = g;
        let sum = self.get(g)
            + self.get([x - 1, y, z])
            + self.get([x + 1, y, z])
            + self.get([x, y - 1, z])
            + self.get([x, y + 1, z])
            + self.get([x, y, z - 1])
            + self.get([x, y, z + 1]);
        (sum + NEIGHBOURS / 2).div_euclid(NEIGHBOURS)
    }
}

/// Replaces every sample from a field in world units, positive solid: the Fill
/// tool, for interiors that start from a formula. Returns the chunks to remesh.
pub fn fill(
    volume: &mut Volume,
    ctx: &mut Context,
    field: impl Fn(Vec3d) -> f64,
    material: u8,
) -> Vec<[u16; 3]> {
    if ctx.lattice != volume.lattice {
        return Vec::new();
    }
    let span = Span {
        first: [0; 3],
        end: volume.lattice.samples(),
    };
    let cell = volume.lattice.cell.units();
    edit_samples(volume, ctx, span, &mut |_, p, _, _| {
        (quantize(field(p), cell), material)
    })
}
