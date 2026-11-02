//! One bounce for voxel cells. Each chunk with a surface becomes one patch, as
//! a face does, and cells gather from every patch of the level. A ray per cell
//! and patch would be far too many, so cells gather in blocks of 4x4x4: one ray
//! from the block's middle decides which patches the whole block sees, and the
//! result is blurred over neighbouring cells so block edges leave no steps.

use mscent_math::Vec3;
use mscent_math::epsilon::VOXEL_LIGHT_LIFT_CELLS;

use super::bake::{Scene, luminance};
use super::bounce::{Patch, transfer};
use super::cell_sites::CellSites;
use super::cells::Direct;
use super::parallel::par_map;

/// Cells along one edge of a gathering block.
const BLOCK: i32 = 4;

/// Cells blurred per unit of work handed to a thread.
const BATCH: usize = 256;

/// Light and direction gathered by one cell.
type Gathered = (Vec3, Vec3);

/// Adds the bounce light of `faces` and of the chunks' own patches to the lit
/// entries of `direct`, which is aligned with the sites.
pub(super) fn add_bounce(s: &Scene, sites: &CellSites, faces: &[Patch], direct: &mut [Direct]) {
    if s.cfg.albedo <= 0.0 || s.cfg.bounce_gain <= 0.0 || sites.is_empty() {
        return;
    }
    let (chunks, chunk_of) = chunk_patches(s, sites, direct);
    let patches: Vec<&Patch> = faces.iter().chain(&chunks).collect();
    if patches.iter().all(|p| luminance(p.radiosity) <= 1e-6) {
        return;
    }
    let raw = gather_blocks(s, sites, direct, &patches, faces.len(), &chunk_of);
    if s.cancelled() {
        return;
    }
    let smooth = blur(s, sites, direct, &raw);
    for (d, (c, dir)) in direct.iter_mut().zip(smooth) {
        if let Direct::Lit { color, dir: d_dir } = d {
            *color = *color + c;
            *d_dir = *d_dir + dir;
        }
    }
}

/// A patch for each run of sites in one chunk, and the patch each site is in.
fn chunk_patches(s: &Scene, sites: &CellSites, direct: &[Direct]) -> (Vec<Patch>, Vec<u32>) {
    let mut patches = Vec::new();
    let mut chunk_of = vec![0u32; sites.len()];
    let mut start = 0;
    while start < sites.len() {
        let (volume, chunk) = (sites.key(start).volume, sites.key(start).chunk);
        let end = (start..sites.len())
            .find(|&i| (sites.key(i).volume, sites.key(i).chunk) != (volume, chunk))
            .unwrap_or(sites.len());
        chunk_of[start..end].fill(patches.len() as u32);
        patches.push(patch_of(s, sites, direct, start..end));
        start = end;
    }
    (patches, chunk_of)
}

/// The patch for the sites in `run`: the average direct light of the lit ones,
/// sent from a point in front of the one nearest their middle.
fn patch_of(s: &Scene, sites: &CellSites, direct: &[Direct], run: std::ops::Range<usize>) -> Patch {
    let lit: Vec<(usize, Vec3)> = run
        .clone()
        .filter_map(|i| match direct[i] {
            Direct::Lit { color, .. } => Some((i, color)),
            Direct::Solid => None,
        })
        .collect();
    let n = lit.len().max(1) as f32;
    let avg = lit.iter().fold(Vec3::ZERO, |a, &(_, c)| a + c) * (1.0 / n);
    let mid = lit.iter().fold(Vec3::ZERO, |a, &(i, _)| a + sites.pos(i)) * (1.0 / n);
    let nearest = lit.iter().map(|&(i, _)| i).min_by(|&a, &b| {
        let d = |i: usize| (sites.pos(i) - mid).length_squared();
        d(a).total_cmp(&d(b))
    });
    let (centre, size) = nearest.map_or((mid, 1.0), |i| {
        let lift = VOXEL_LIGHT_LIFT_CELLS * sites.cell_size(i);
        (sites.pos(i) + sites.normal(i) * lift, sites.cell_size(i))
    });
    Patch {
        centre,
        normal: Vec3::ZERO,
        area: run.len() as f32 * size * size,
        radiosity: avg * s.cfg.albedo,
        omni: true,
    }
}

/// The sites grouped by block: runs of indices into the site list.
fn blocks(sites: &CellSites) -> Vec<Vec<usize>> {
    let mut order: Vec<(u32, [i32; 3], usize)> = (0..sites.len())
        .map(|i| {
            let key = sites.key(i);
            (key.volume, key.global().map(|g| g.div_euclid(BLOCK)), i)
        })
        .collect();
    order.sort_unstable();
    let mut out: Vec<Vec<usize>> = Vec::new();
    let mut last = None;
    for (v, b, i) in order {
        if last != Some((v, b)) {
            out.push(Vec::new());
            last = Some((v, b));
        }
        if let Some(run) = out.last_mut() {
            run.push(i);
        }
    }
    out
}

/// What each lit site gathers, with one ray per block and patch.
fn gather_blocks(
    s: &Scene,
    sites: &CellSites,
    direct: &[Direct],
    patches: &[&Patch],
    face_count: usize,
    chunk_of: &[u32],
) -> Vec<Gathered> {
    let blocks = blocks(sites);
    let parts = par_map(s.cfg.workers, blocks.len(), |b| {
        if s.cancelled() {
            return Vec::new();
        }
        let lit: Vec<usize> = blocks[b]
            .iter()
            .copied()
            .filter(|&i| matches!(direct[i], Direct::Lit { .. }))
            .collect();
        gather_block(s, sites, patches, face_count, chunk_of, &lit)
    });
    let mut raw = vec![(Vec3::ZERO, Vec3::ZERO); sites.len()];
    for (i, g) in parts.into_iter().flatten() {
        raw[i] = g;
    }
    raw
}

fn gather_block(
    s: &Scene,
    sites: &CellSites,
    patches: &[&Patch],
    face_count: usize,
    chunk_of: &[u32],
    lit: &[usize],
) -> Vec<(usize, Gathered)> {
    let Some(&first) = lit.first() else {
        return Vec::new();
    };
    // Rays leave from the cell nearest the block's middle, lifted off its surface.
    let mid = lit.iter().fold(Vec3::ZERO, |a, &i| a + sites.pos(i)) * (1.0 / lit.len() as f32);
    let rep = lit.iter().copied().min_by(|&a, &b| {
        let d = |i: usize| (sites.pos(i) - mid).length_squared();
        d(a).total_cmp(&d(b))
    });
    let rep = rep.unwrap_or(first);
    let lift = VOXEL_LIGHT_LIFT_CELLS * sites.cell_size(rep);
    let from = (sites.pos(rep) + sites.normal(rep) * lift).to_f64();
    // A block lies inside one chunk; a chunk does not light itself.
    let own = face_count + chunk_of[first] as usize;
    let mut acc = vec![(Vec3::ZERO, Vec3::ZERO); lit.len()];
    let mut got: Vec<Option<Gathered>> = Vec::with_capacity(lit.len());
    let others = patches.iter().enumerate().filter(|&(j, _)| j != own);
    for (_, patch) in others {
        got.clear();
        got.extend(
            lit.iter()
                .map(|&i| transfer(s, patch, sites.pos(i), sites.normal(i))),
        );
        if got.iter().all(Option::is_none) {
            continue;
        }
        let target = (patch.centre + patch.normal * s.cfg.normal_bias).to_f64();
        if !s.occ.segment_clear(from, target) {
            continue;
        }
        for (a, g) in acc.iter_mut().zip(&got) {
            if let Some((c, to_patch)) = *g {
                a.0 = a.0 + c;
                a.1 = a.1 + to_patch * luminance(c);
            }
        }
    }
    lit.iter().copied().zip(acc).collect()
}

/// `raw` averaged with the lit cells around each one, so the bounce has no
/// edge where one block's visibility changes to the next's.
fn blur(s: &Scene, sites: &CellSites, direct: &[Direct], raw: &[Gathered]) -> Vec<Gathered> {
    let batches = sites.len().div_ceil(BATCH);
    let parts = par_map(s.cfg.workers, batches, |b| {
        let range = b * BATCH..((b + 1) * BATCH).min(sites.len());
        range
            .map(|i| blurred(sites, direct, raw, i))
            .collect::<Vec<_>>()
    });
    parts.into_iter().flatten().collect()
}

fn blurred(sites: &CellSites, direct: &[Direct], raw: &[Gathered], i: usize) -> Gathered {
    let (mut c, mut d, mut weight) = (Vec3::ZERO, Vec3::ZERO, 0.0f32);
    for dz in -1..=1 {
        for dy in -1..=1 {
            for dx in -1..=1 {
                let Some(j) = sites.neighbour(i, [dx, dy, dz]) else {
                    continue;
                };
                if matches!(direct[j], Direct::Lit { .. }) {
                    // The cell itself counts double.
                    let w = if j == i { 2.0 } else { 1.0 };
                    c = c + raw[j].0 * w;
                    d = d + raw[j].1 * w;
                    weight += w;
                }
            }
        }
    }
    if weight > 0.0 {
        (c * (1.0 / weight), d * (1.0 / weight))
    } else {
        (Vec3::ZERO, Vec3::ZERO)
    }
}
