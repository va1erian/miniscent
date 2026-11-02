//! The resolved samples one chunk's mesh needs: its own, plus two samples of
//! padding on every side from its neighbours or the ring. Two chunks fill a
//! shared sample with the same value, which is why their meshes meet exactly.
//!
//! Inside a volume one sample would do. At its border the second is what lets
//! a surface run past the box on every side: a cell needs both its samples, and
//! a quad needs all four of its cells.

use mscent_math::Vec3d;

use super::chunk::sample_index;
use super::context::{Context, apply_cuts};
use super::volume::Volume;
use super::{CHUNK_EDGE, edited};
use crate::brush::Op;

/// Samples along one edge of a block.
pub(super) const BLOCK_EDGE: usize = CHUNK_EDGE + 4;
pub(super) const BLOCK_SAMPLES: usize = BLOCK_EDGE * BLOCK_EDGE * BLOCK_EDGE;

/// Index of a local sample, each coordinate in `-2..=17`.
pub(super) const fn block_index(x: i32, y: i32, z: i32) -> usize {
    let e = BLOCK_EDGE as i32;
    ((x + 2) + (y + 2) * e + (z + 2) * e * e) as usize
}

pub(super) struct Block {
    pub density: [i8; BLOCK_SAMPLES],
    pub material: [u8; BLOCK_SAMPLES],
}

impl Block {
    pub fn new() -> Box<Self> {
        Box::new(Self {
            density: [0; BLOCK_SAMPLES],
            material: [0; BLOCK_SAMPLES],
        })
    }
}

/// Local samples of one neighbour along one axis: `(neighbour offset, first,
/// end)`.
const REGIONS: [(i32, i32, i32); 3] = [
    (-1, -2, 0),
    (0, 0, CHUNK_EDGE as i32),
    (1, CHUNK_EDGE as i32, CHUNK_EDGE as i32 + 2),
];

/// Fills `out` with the resolved samples around chunk `coord`.
pub(super) fn fill_block(volume: &Volume, ctx: &mut Context, coord: [i32; 3], out: &mut Block) {
    for rz in REGIONS {
        for ry in REGIONS {
            for rx in REGIONS {
                fill_region(volume, ctx, coord, [rx, ry, rz], out);
            }
        }
    }
}

/// One of the 27 parts of a block: all of it lies in a single neighbour chunk
/// or all of it in the ring.
fn fill_region(
    volume: &Volume,
    ctx: &mut Context,
    coord: [i32; 3],
    r: [(i32, i32, i32); 3],
    out: &mut Block,
) {
    let neighbour = [coord[0] + r[0].0, coord[1] + r[1].0, coord[2] + r[2].0];
    match ctx.lattice.chunk_index(neighbour) {
        Some(index) => fill_from_chunk(volume, ctx, coord, neighbour, index, r, out),
        None => fill_from_ring(volume, ctx, coord, r, out),
    }
}

fn fill_from_chunk(
    volume: &Volume,
    ctx: &mut Context,
    coord: [i32; 3],
    neighbour: [i32; 3],
    index: usize,
    r: [(i32, i32, i32); 3],
    out: &mut Block,
) {
    let edge = CHUNK_EDGE as i32;
    let lattice = ctx.lattice;
    let cell = lattice.cell.units();
    let origin = coord.map(|c| c * edge);
    let global = |x: i32, y: i32, z: i32| [origin[0] + x, origin[1] + y, origin[2] + z];
    let lo = lattice.position(global(r[0].1, r[1].1, r[2].1));
    let hi = lattice.position(global(r[0].2 - 1, r[1].2 - 1, r[2].2 - 1));
    ctx.ensure_base(index, neighbour);
    let cuts = ctx.cuts_near(lo, hi);
    let Some(base) = ctx.cached_base(index) else {
        return;
    };
    let chunk = &volume.table[index];
    for z in r[2].1..r[2].2 {
        for y in r[1].1..r[1].2 {
            for x in r[0].1..r[0].2 {
                let (nx, ny, nz) = (
                    (x - r[0].0 * edge) as usize,
                    (y - r[1].0 * edge) as usize,
                    (z - r[2].0 * edge) as usize,
                );
                let i = sample_index(nx, ny, nz);
                let (delta, stored) = chunk.sample(i);
                let value = edited(base.get(i), delta);
                let k = block_index(x, y, z);
                out.density[k] = if cuts.is_empty() {
                    value
                } else {
                    apply_cuts(&cuts, value, lattice.position(global(x, y, z)), cell)
                };
                out.material[k] = base.material(i, stored);
            }
        }
    }
}

fn fill_from_ring(
    volume: &Volume,
    ctx: &mut Context,
    coord: [i32; 3],
    r: [(i32, i32, i32); 3],
    out: &mut Block,
) {
    let edge = CHUNK_EDGE as i32;
    let lattice = ctx.lattice;
    let samples = lattice.samples();
    let origin = coord.map(|c| c * edge);
    let global = |x: i32, y: i32, z: i32| [origin[0] + x, origin[1] + y, origin[2] + z];
    let lo = lattice.position(global(r[0].1, r[1].1, r[2].1));
    let hi = lattice.position(global(r[0].2 - 1, r[1].2 - 1, r[2].2 - 1));
    let each = || {
        (r[2].1..r[2].2).flat_map(move |z| {
            (r[1].1..r[1].2).flat_map(move |y| (r[0].1..r[0].2).map(move |x| [x, y, z]))
        })
    };
    for [x, y, z] in each() {
        out.material[block_index(x, y, z)] =
            nearest_material(volume, ctx, global(x, y, z), samples);
    }
    let world = ctx.world_near(lo, hi);
    for [x, y, z] in each() {
        let at = lattice.position(global(x, y, z));
        out.density[block_index(x, y, z)] = ctx.ring_sample(&world, at);
    }
    if ctx.has_neighbours() {
        for [x, y, z] in each() {
            let at = lattice.position(global(x, y, z));
            overlay_neighbour(ctx, at, block_index(x, y, z), out);
        }
    }
}

/// A subtract volume beside this one owns its box outright, and an add volume
/// only ever adds to what the brushes put there.
fn overlay_neighbour(ctx: &mut Context, at: Vec3d, k: usize, out: &mut Block) {
    let Some((value, material, op)) = ctx.neighbour_sample(at) else {
        return;
    };
    if op == Op::Subtract || value > out.density[k] {
        out.density[k] = value;
        out.material[k] = material;
    }
}

/// The material of the interior sample closest to ring sample `g`, so a
/// surface closing against the ring keeps the look of what it closes.
fn nearest_material(volume: &Volume, ctx: &mut Context, g: [i32; 3], samples: [i32; 3]) -> u8 {
    let edge = CHUNK_EDGE as i32;
    let at = [0, 1, 2].map(|axis| g[axis].clamp(0, samples[axis] - 1));
    let local = at.map(|v| v.rem_euclid(edge) as usize);
    let coord = at.map(|v| v.div_euclid(edge));
    let Some(index) = volume.lattice.chunk_index(coord) else {
        return 0;
    };
    let i = sample_index(local[0], local[1], local[2]);
    ctx.ensure_base(index, coord);
    let stored = volume.table.get(index).map_or(0, |c| c.sample(i).1);
    ctx.cached_base(index).map_or(0, |b| b.material(i, stored))
}
