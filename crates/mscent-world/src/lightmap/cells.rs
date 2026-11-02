//! Light on voxel surfaces: one record per surface cell, keyed by the cell's
//! place in its volume, so every chunk that draws a vertex in that cell reads
//! the same colour and direction and neighbouring chunks shade identically.
//!
//! A cell is lit like a luxel: from a point lifted off the surface (here
//! [`VOXEL_LIGHT_LIFT_CELLS`] cells along the normal), with the same soft shadow
//! sampling. A cell whose lifted point is inside solid brush (a stalagmite's
//! root, with its normal leaning into the floor) would be black, so it takes
//! its value from the lit cells around it instead.

use mscent_math::Vec3;
use mscent_math::epsilon::VOXEL_LIGHT_LIFT_CELLS;
use std::sync::atomic::{AtomicBool, Ordering};

use super::bake::luminance;
use super::occluders::Occluders;
use super::parallel::par_map;
use super::soft::{Footprint, Receiver, light_at};
use super::{BakeConfig, PointLight};
use crate::voxel::VoxelWorld;

use super::cell_sites::CellSites;

/// Bytes of a record in a cache file.
pub const RECORD_BYTES: usize = 9;

/// Steps of light in a record's colour: 1.0, the plain texture, is this many.
/// The most it holds is 32, far past where the shader's soft limit has
/// flattened a colour.
const COLOR_STEPS: f32 = 2048.0;
const DIR_STEPS: f32 = 127.0;

/// Cells a hole in the light is filled from: a lit cell this far away or less.
const FILL_REACH: i32 = 2;

/// Cells lit per unit of work handed to a thread.
const BATCH: usize = 64;

/// Light at one surface cell: 9 bytes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CellRecord {
    color: [u16; 3],
    dir: [i8; 3],
}

impl CellRecord {
    /// Quantises `color` (linear light, as a luxel holds it) and `dir`, a world
    /// vector towards the light of any length, whose direction is kept.
    pub fn new(color: Vec3, dir: Vec3) -> Self {
        let q = |c: f32| (c * COLOR_STEPS).round().clamp(0.0, f32::from(u16::MAX)) as u16;
        let d = |c: f32| (c * DIR_STEPS).round().clamp(-DIR_STEPS, DIR_STEPS) as i8;
        let unit = dir.normalized().unwrap_or(Vec3::ZERO);
        Self {
            color: [q(color.x), q(color.y), q(color.z)],
            dir: [d(unit.x), d(unit.y), d(unit.z)],
        }
    }

    /// Linear light, 1.0 being a lamp-lit plain texture.
    pub fn color(self) -> Vec3 {
        let c = self.color.map(|v| f32::from(v) / COLOR_STEPS);
        Vec3::new(c[0], c[1], c[2])
    }

    /// World direction towards the light, nearly unit length, or zero where no
    /// light arrives and the surface normal is the better guess.
    pub fn dir(self) -> Vec3 {
        let d = self.dir.map(|v| f32::from(v) / DIR_STEPS);
        Vec3::new(d[0], d[1], d[2])
    }

    pub fn to_bytes(self) -> [u8; RECORD_BYTES] {
        let mut out = [0; RECORD_BYTES];
        for (i, c) in self.color.iter().enumerate() {
            out[i * 2..i * 2 + 2].copy_from_slice(&c.to_le_bytes());
        }
        for (i, d) in self.dir.iter().enumerate() {
            out[6 + i] = d.to_le_bytes()[0];
        }
        out
    }

    /// Every byte pattern is a record: the decoder cannot fail or produce a
    /// number outside its ranges.
    pub fn from_bytes(b: &[u8; RECORD_BYTES]) -> Self {
        Self {
            color: [0, 1, 2].map(|i| u16::from_le_bytes([b[i * 2], b[i * 2 + 1]])),
            dir: [0, 1, 2].map(|i| i8::from_le_bytes([b[6 + i]])),
        }
    }
}

/// What lights cells: the occluders, the lamps and the settings.
pub struct CellLighting<'a> {
    pub occ: &'a Occluders<'a>,
    pub lights: &'a [PointLight],
    pub cfg: &'a BakeConfig,
}

/// One cell before its holes are filled.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Direct {
    /// The lifted point is inside solid brush.
    Solid,
    /// Light summed over the lamps, and the world directions towards them
    /// weighted by how much light each brings.
    Lit { color: Vec3, dir: Vec3 },
}

/// The two world axes tangent to `n`.
fn tangent_frame(n: Vec3) -> (Vec3, Vec3) {
    let side = if n.x.abs() < 0.9 {
        Vec3::new(1.0, 0.0, 0.0)
    } else {
        Vec3::new(0.0, 1.0, 0.0)
    };
    let t = n
        .cross(side)
        .normalized()
        .unwrap_or(Vec3::new(0.0, 1.0, 0.0));
    (t, n.cross(t))
}

/// Direct light of site `i`: lamps and shadows, no ambient and no bounce.
pub(super) fn direct_cell(env: &CellLighting, sites: &CellSites, i: usize) -> Direct {
    let (pos, normal) = (sites.pos(i), sites.normal(i));
    let size = sites.cell_size(i);
    let lift = VOXEL_LIGHT_LIFT_CELLS * size;
    if env.occ.in_brush_solid((pos + normal * lift).to_f64()) {
        return Direct::Solid;
    }
    let (tangent, bitangent) = tangent_frame(normal);
    let rx = Receiver {
        occ: env.occ,
        foot: Footprint::Cell {
            normal,
            tangent,
            bitangent,
            size,
        },
        bias: lift,
        centre: pos,
    };
    let (mut color, mut dir) = (Vec3::ZERO, Vec3::ZERO);
    for light in env.lights {
        if let Some((c, to_light)) = light_at(&rx, light, env.cfg) {
            color = color + c;
            dir = dir + to_light * luminance(c);
        }
    }
    Direct::Lit { color, dir }
}

/// Direct light of the sites in `which`, in that order. Stops early, leaving
/// fewer results, when `cancel` is set.
pub(super) fn direct_pass(
    env: &CellLighting,
    sites: &CellSites,
    which: &[usize],
    cancel: &AtomicBool,
) -> Vec<Direct> {
    let batches = which.len().div_ceil(BATCH);
    let parts = par_map(env.cfg.workers, batches, |b| {
        if cancel.load(Ordering::Relaxed) {
            return Vec::new();
        }
        let from = b * BATCH;
        let ids = &which[from..(from + BATCH).min(which.len())];
        ids.iter().map(|&i| direct_cell(env, sites, i)).collect()
    });
    parts.into_iter().flatten().collect()
}

/// The records of `which`: holes filled from lit neighbours, ambient added and
/// everything quantised. `direct` is [`direct_pass`] over the same `which`.
pub(super) fn settle(
    env: &CellLighting,
    sites: &CellSites,
    which: &[usize],
    direct: &[Direct],
) -> Vec<CellRecord> {
    let mut slot = vec![u32::MAX; sites.len()];
    for (k, &i) in which.iter().enumerate() {
        if let Some(s) = slot.get_mut(i) {
            *s = k as u32;
        }
    }
    // A neighbour outside `which` (a chunk lit alone) is lit on the spot.
    let known = |i: usize| match slot.get(i).copied() {
        Some(k) if k != u32::MAX => direct.get(k as usize).copied().unwrap_or(Direct::Solid),
        _ => direct_cell(env, sites, i),
    };
    which
        .iter()
        .zip(direct)
        .map(|(&i, &d)| {
            let (color, dir) = match d {
                Direct::Lit { color, dir } => (color, dir),
                Direct::Solid => filled(sites, i, &known),
            };
            CellRecord::new(color + env.cfg.ambient, dir)
        })
        .collect()
}

/// The weighted mean of the lit cells within [`FILL_REACH`] of site `i`, nearer
/// ones counting more. Nothing lit nearby leaves it dark.
fn filled(sites: &CellSites, i: usize, known: &impl Fn(usize) -> Direct) -> (Vec3, Vec3) {
    let (mut color, mut dir, mut weight) = (Vec3::ZERO, Vec3::ZERO, 0.0f32);
    for dz in -FILL_REACH..=FILL_REACH {
        for dy in -FILL_REACH..=FILL_REACH {
            for dx in -FILL_REACH..=FILL_REACH {
                let Some(j) = sites.neighbour(i, [dx, dy, dz]).filter(|&j| j != i) else {
                    continue;
                };
                if let Direct::Lit { color: c, dir: d } = known(j) {
                    let w = 1.0 / (1 + dx * dx + dy * dy + dz * dz) as f32;
                    color = color + c * w;
                    dir = dir + d * w;
                    weight += w;
                }
            }
        }
    }
    if weight > 0.0 {
        (color * (1.0 / weight), dir * (1.0 / weight))
    } else {
        (Vec3::ZERO, Vec3::ZERO)
    }
}

/// Lights the sites in `which` alone: direct light with the lamps' soft shadow
/// settings and no bounce. This is what refining one chunk after an edit calls;
/// [`CellSites::of_chunk`] names a chunk's sites.
pub fn light_cells(env: &CellLighting, sites: &CellSites, which: &[usize]) -> Vec<CellRecord> {
    let direct = direct_pass(env, sites, which, &AtomicBool::new(false));
    settle(env, sites, which, &direct)
}

impl CellSites {
    /// Sites a chunk's full-detail mesh reads, including border cells whose
    /// record another chunk owns.
    pub fn of_chunk(&self, world: &VoxelWorld, chunk: usize) -> Vec<usize> {
        let Some(c) = world.chunks.get(chunk) else {
            return Vec::new();
        };
        let mut out: Vec<usize> = c.lods[0]
            .vertices
            .iter()
            .filter_map(|v| self.lookup(chunk, v.cell))
            .collect();
        out.sort_unstable();
        out.dedup();
        out
    }
}
