//! What sculpting needs beyond a dab: moving and growing a box, turning a volume
//! into its other kind, looking along a ray and clearing crumbs.

use std::collections::HashSet;
use std::sync::Arc;

use mscent_math::Vec3d;

use super::chunk::{Chunk, sample_index};
use super::context::Context;
use super::volume::{Lattice, Volume, VolumeError};
use super::{CHUNK_EDGE, MAX_CHUNKS_PER_AXIS, MAX_SAMPLE, edited, quantize};
use crate::brush::Op;

/// Fewest connected solid samples that stay when a stroke ends.
pub const MIN_ISLAND_SAMPLES: usize = 8;

impl Volume {
    /// The same volume `cells` further on: its sculpting moves with it and its
    /// walls re-form from what lies there.
    pub fn translated(&self, cells: [i32; 3]) -> Result<Self, VolumeError> {
        let mut origin = [0; 3];
        for a in 0..3 {
            origin[a] = self.lattice.origin[a]
                .checked_add(cells[a])
                .ok_or(VolumeError::OutOfRange)?;
        }
        let moved = Volume::new(
            self.lattice.op,
            self.lattice.order,
            self.lattice.cell,
            origin,
            self.lattice.chunks,
        )?;
        Ok(Self {
            lattice: moved.lattice,
            table: Arc::clone(&self.table),
        })
    }

    /// The smallest box on this one's chunk grid that holds `lo..hi` as well, with
    /// the chunks it already has in the same places. `None` when it already does.
    pub fn grown_to_hold(&self, lo: Vec3d, hi: Vec3d) -> Result<Option<Self>, VolumeError> {
        let l = self.lattice;
        let size = l.cell.units() * CHUNK_EDGE as f64;
        let (lo, hi) = ([lo.x, lo.y, lo.z], [hi.x, hi.y, hi.z]);
        let (mut before, mut after) = ([0i64; 3], [0i64; 3]);
        for a in 0..3 {
            let start = f64::from(l.origin[a]) * l.cell.units();
            let last = start + f64::from(l.chunks[a]) * size;
            before[a] = ((start - lo[a]) / size).ceil().max(0.0) as i64;
            after[a] = ((hi[a] - last) / size).ceil().max(0.0) as i64;
        }
        if before == [0; 3] && after == [0; 3] {
            return Ok(None);
        }
        self.extended(before, after).map(Some)
    }

    /// This volume with `before` and `after` more chunks along each axis.
    pub fn extended(&self, before: [i64; 3], after: [i64; 3]) -> Result<Self, VolumeError> {
        let l = self.lattice;
        let mut origin = [0i32; 3];
        let mut chunks = [0u16; 3];
        for a in 0..3 {
            let count = i64::from(l.chunks[a]) + before[a] + after[a];
            if count > i64::from(MAX_CHUNKS_PER_AXIS) {
                return Err(VolumeError::BadSize);
            }
            chunks[a] = u16::try_from(count).map_err(|_| VolumeError::BadSize)?;
            let shifted = i64::from(l.origin[a]) - before[a] * CHUNK_EDGE as i64;
            origin[a] = i32::try_from(shifted).map_err(|_| VolumeError::OutOfRange)?;
        }
        let mut grown = Volume::new(l.op, l.order, l.cell, origin, chunks)?;
        let table = Arc::make_mut(&mut grown.table);
        let [cx, cy, cz] = l.chunks.map(i32::from);
        for z in 0..cz {
            for y in 0..cy {
                for x in 0..cx {
                    let at = [
                        x + before[0] as i32,
                        y + before[1] as i32,
                        z + before[2] as i32,
                    ];
                    let (Some(from), Some(to)) =
                        (l.chunk_index([x, y, z]), grown.lattice.chunk_index(at))
                    else {
                        continue;
                    };
                    table[to] = self.table[from].clone();
                }
            }
        }
        Ok(grown)
    }

    /// Whether the two have the same box and kind, whatever they hold.
    pub fn same_box(&self, other: &Volume) -> bool {
        let (a, b) = (self.lattice, other.lattice);
        a.op == b.op && a.cell == b.cell && a.origin == b.origin && a.chunks == b.chunks
    }

    /// Whether the fields match, materials aside: what light depends on. Order
    /// counts only through the brushes around it, so it is compared elsewhere.
    pub fn same_shape(&self, other: &Volume) -> bool {
        self.same_box(other)
            && (Arc::ptr_eq(&self.table, &other.table)
                || self
                    .table
                    .iter()
                    .zip(other.table.iter())
                    .all(|(a, b)| a.same_deltas(b)))
    }

    /// The chunks whose mesh can differ from `older`'s, a volume with the same
    /// box: those holding a changed sample and those holding it as padding.
    pub fn changed_chunks(&self, older: &Volume) -> Vec<[u16; 3]> {
        let l = self.lattice;
        let [cx, cy, cz] = l.chunks.map(i32::from);
        let mut out = HashSet::new();
        for z in 0..cz {
            for y in 0..cy {
                for x in 0..cx {
                    let Some(i) = l.chunk_index([x, y, z]) else {
                        continue;
                    };
                    let same = match (self.table.get(i), older.table.get(i)) {
                        (Some(a), Some(b)) => a == b,
                        _ => false,
                    };
                    if !same {
                        add_with_neighbours(&l, [x, y, z], &mut out);
                    }
                }
            }
        }
        let mut list: Vec<[u16; 3]> = out.into_iter().collect();
        list.sort_unstable();
        list
    }

    /// All the volume's chunks.
    pub fn all_chunks(&self) -> Vec<[u16; 3]> {
        let [cx, cy, cz] = self.lattice.chunks;
        let mut out = Vec::with_capacity(self.lattice.chunk_count());
        for z in 0..cz {
            for y in 0..cy {
                for x in 0..cx {
                    out.push([x, y, z]);
                }
            }
        }
        out
    }

    /// Whether a surface in this volume reads samples of `other`.
    pub fn touches(&self, other: &Volume) -> bool {
        super::context::reaches(self, other)
    }

    /// Whether `p` is in the box.
    pub fn holds(&self, p: Vec3d) -> bool {
        let (lo, hi) = self.bounds();
        p.x >= lo.x && p.x < hi.x && p.y >= lo.y && p.y < hi.y && p.z >= lo.z && p.z < hi.z
    }

    /// The same sculpted result as the other kind of volume. `old` is this
    /// volume's context and `new` one built for [`Self::with_op`]'s result.
    pub fn converted(&self, op: Op, old: &mut Context, new: &mut Context) -> Self {
        let mut out = self.clone();
        out.lattice.op = op;
        if op == self.lattice.op {
            return out;
        }
        let l = self.lattice;
        let table = Arc::make_mut(&mut out.table);
        let [cx, cy, cz] = l.chunks.map(i32::from);
        for z in 0..cz {
            for y in 0..cy {
                for x in 0..cx {
                    let c = [x, y, z];
                    let Some(i) = l.chunk_index(c) else { continue };
                    old.ensure_base(i, c);
                    new.ensure_base(i, c);
                    if let (Some(a), Some(b)) = (old.cached_base(i), new.cached_base(i)) {
                        rewrite_chunk(&mut table[i], a, b);
                    }
                }
            }
        }
        out
    }

    /// The same volume as `op` with no rewriting: for building its new context.
    pub fn with_op(&self, op: Op) -> Self {
        let mut out = self.clone();
        out.lattice.op = op;
        out
    }
}

fn rewrite_chunk(
    chunk: &mut Chunk,
    old: &super::context::BaseChunk,
    new: &super::context::BaseChunk,
) {
    let data = chunk.data_mut();
    for i in 0..data.delta.len() {
        let value = edited(old.get(i), data.delta[i]);
        data.delta[i] = (i16::from(value) - i16::from(new.get(i))) as i8;
    }
    chunk.normalize();
}

fn add_with_neighbours(l: &Lattice, c: [i32; 3], out: &mut HashSet<[u16; 3]>) {
    for dz in -1..=1 {
        for dy in -1..=1 {
            for dx in -1..=1 {
                let n = [c[0] + dx, c[1] + dy, c[2] + dz];
                if l.chunk_index(n).is_some() {
                    out.insert(n.map(|v| v as u16));
                }
            }
        }
    }
}

/// Where a ray first meets solid in `volume`, as a distance along the unit `dir`,
/// within half a cell. `None` if it does not, or it starts in solid and never
/// leaves it, which is not a surface a pointer can be on.
pub fn first_solid(
    volume: &Volume,
    ctx: &mut Context,
    origin: Vec3d,
    dir: Vec3d,
    max: f64,
) -> Option<f64> {
    if ctx.lattice != volume.lattice {
        return None;
    }
    let (lo, hi) = volume.bounds();
    let (start, end) = slab(origin, dir, lo, hi, max)?;
    let step = volume.lattice.cell.units() * 0.5;
    let mut t = start;
    let mut left_solid = !is_solid(volume, ctx, origin + dir * t);
    while t <= end {
        let solid = is_solid(volume, ctx, origin + dir * t);
        if solid && left_solid {
            return Some(t);
        }
        left_solid |= !solid;
        t += step;
    }
    None
}

/// The part of a ray inside a box.
fn slab(origin: Vec3d, dir: Vec3d, lo: Vec3d, hi: Vec3d, max: f64) -> Option<(f64, f64)> {
    let (mut t0, mut t1) = (0.0f64, max);
    for (o, d, a, b) in [
        (origin.x, dir.x, lo.x, hi.x),
        (origin.y, dir.y, lo.y, hi.y),
        (origin.z, dir.z, lo.z, hi.z),
    ] {
        if d.abs() < 1e-12 {
            if o < a || o > b {
                return None;
            }
            continue;
        }
        let (mut near, mut far) = ((a - o) / d, (b - o) / d);
        if near > far {
            std::mem::swap(&mut near, &mut far);
        }
        t0 = t0.max(near);
        t1 = t1.min(far);
    }
    (t0 <= t1).then_some((t0, t1))
}

fn is_solid(volume: &Volume, ctx: &mut Context, p: Vec3d) -> bool {
    let l = volume.lattice;
    let own = l.cell.units();
    let g = [
        (p.x / own).round() as i32 - l.origin[0],
        (p.y / own).round() as i32 - l.origin[1],
        (p.z / own).round() as i32 - l.origin[2],
    ];
    resolved_at(volume, ctx, g).is_some_and(|v| v >= 0)
}

/// The resolved sample `g` of the volume, if it has one.
fn resolved_at(volume: &Volume, ctx: &mut Context, g: [i32; 3]) -> Option<i8> {
    let l = volume.lattice;
    let edge = CHUNK_EDGE as i32;
    let coord = g.map(|v| v.div_euclid(edge));
    let index = l.chunk_index(coord)?;
    let local = g.map(|v| v.rem_euclid(edge) as usize);
    let i = sample_index(local[0], local[1], local[2]);
    ctx.ensure_base(index, coord);
    let base = ctx.cached_base(index)?.get(i);
    let (delta, _) = volume.table.get(index)?.sample(i);
    let value = edited(base, delta);
    let at = l.position(g);
    let cuts = ctx.cuts_near(at, at);
    Some(super::context::apply_cuts(&cuts, value, at, l.cell.units()))
}

/// Clears solid samples in `chunks` that belong to a group of fewer than
/// [`MIN_ISLAND_SAMPLES`]: floating crumbs cost triangles and look like bugs. A
/// group reaching the box's border is not a crumb, as it may go on outside.
/// Returns whether anything changed.
pub fn remove_islands(volume: &mut Volume, ctx: &mut Context, chunks: &[[u16; 3]]) -> bool {
    if ctx.lattice != volume.lattice {
        return false;
    }
    let mut crumbs = HashSet::new();
    let mut seen = HashSet::new();
    let edge = CHUNK_EDGE as i32;
    for c in chunks {
        let first = c.map(|v| i32::from(v) * edge);
        for z in 0..edge {
            for y in 0..edge {
                for x in 0..edge {
                    let g = [first[0] + x, first[1] + y, first[2] + z];
                    if !seen.contains(&g) && resolved_at(volume, ctx, g).is_some_and(|v| v >= 0) {
                        visit_group(volume, ctx, g, &mut seen, &mut crumbs);
                    }
                }
            }
        }
    }
    clear_samples(volume, ctx, &crumbs)
}

/// Floods the solid group at `start` up to the size that clears it of suspicion.
fn visit_group(
    volume: &Volume,
    ctx: &mut Context,
    start: [i32; 3],
    seen: &mut HashSet<[i32; 3]>,
    crumbs: &mut HashSet<[i32; 3]>,
) {
    let samples = volume.lattice.samples();
    let mut group = vec![start];
    seen.insert(start);
    let mut at = 0;
    let mut small = true;
    while at < group.len() {
        let g = group[at];
        at += 1;
        if (0..3).any(|a| g[a] == 0 || g[a] == samples[a] - 1) || group.len() >= MIN_ISLAND_SAMPLES
        {
            small = false;
            break;
        }
        for axis in 0..3 {
            for step in [-1, 1] {
                let mut n = g;
                n[axis] += step;
                if !group.contains(&n) && resolved_at(volume, ctx, n).is_some_and(|v| v >= 0) {
                    group.push(n);
                }
            }
        }
    }
    seen.extend(group.iter().copied());
    if small {
        crumbs.extend(group);
    }
}

fn clear_samples(volume: &mut Volume, ctx: &mut Context, crumbs: &HashSet<[i32; 3]>) -> bool {
    let edge = CHUNK_EDGE as i32;
    let l = volume.lattice;
    let cell = l.cell.units();
    let table = Arc::make_mut(&mut volume.table);
    for &g in crumbs {
        let coord = g.map(|v| v.div_euclid(edge));
        let Some(index) = l.chunk_index(coord) else {
            continue;
        };
        let local = g.map(|v| v.rem_euclid(edge) as usize);
        let i = sample_index(local[0], local[1], local[2]);
        ctx.ensure_base(index, coord);
        let Some(base) = ctx.cached_base(index) else {
            continue;
        };
        let air = quantize(-cell, cell).max(-MAX_SAMPLE);
        let b = base.get(i);
        table[index].data_mut().delta[i] = (i16::from(air) - i16::from(b)) as i8;
    }
    for c in table.iter_mut() {
        c.normalize();
    }
    !crumbs.is_empty()
}
