//! Which solid samples belong together: clearing crumbs, counting parts.

use std::collections::HashSet;
use std::sync::Arc;

use super::chunk::sample_index;
use super::context::Context;
use super::edit::resolved_at;
use super::volume::Volume;
use super::{CHUNK_EDGE, MAX_SAMPLE, quantize};

/// Fewest connected solid samples that stay when a stroke ends.
pub const MIN_ISLAND_SAMPLES: usize = 8;

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

/// How many separate solid parts the volume holds, joined face to face. For
/// telling whether a shape has come apart.
pub fn solid_parts(volume: &Volume, ctx: &mut Context) -> usize {
    let samples = volume.lattice.samples();
    let mut seen = HashSet::new();
    let mut parts = 0;
    for z in 0..samples[2] {
        for y in 0..samples[1] {
            for x in 0..samples[0] {
                let start = [x, y, z];
                if seen.contains(&start) || !resolved_at(volume, ctx, start).is_some_and(|v| v >= 0)
                {
                    continue;
                }
                parts += 1;
                seen.insert(start);
                let mut work = vec![start];
                while let Some(g) = work.pop() {
                    for axis in 0..3 {
                        for step in [-1, 1] {
                            let mut n = g;
                            n[axis] += step;
                            if !seen.contains(&n)
                                && resolved_at(volume, ctx, n).is_some_and(|v| v >= 0)
                            {
                                seen.insert(n);
                                work.push(n);
                            }
                        }
                    }
                }
            }
        }
    }
    parts
}
