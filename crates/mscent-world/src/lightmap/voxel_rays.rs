//! Shadow rays against the surface of one voxel volume. The triangles of every
//! chunk go into a grid of blocks four cells wide, and a ray visits only the
//! blocks it crosses, so a chunk with no surface costs nothing and a chunk with
//! one costs the few triangles near the ray, not all of them.
//!
//! The grid reaches one chunk past the volume's box on every side, because a
//! border chunk owns triangles in the ring beyond it.

use std::sync::Arc;

use mscent_math::{Vec3, Vec3d, epsilon};

use crate::voxel::{NO_CHUNK, VolumeGeometry, VoxelChunk};

/// Blocks along one edge of a chunk.
const BLOCKS_PER_CHUNK: i32 = 4;

pub(super) struct VolumeRays {
    /// World position of the grid's corner.
    lo: [f64; 3],
    /// World size of a block.
    block: f64,
    /// Blocks along each axis.
    dims: [i32; 3],
    /// Ids of the blocks that hold a triangle, sorted.
    keys: Vec<u32>,
    /// Where each key's triangles start in `members`; one more entry than keys.
    starts: Vec<u32>,
    members: Vec<u32>,
    tris: Vec<[Vec3d; 3]>,
}

fn axes(v: Vec3d) -> [f64; 3] {
    [v.x, v.y, v.z]
}

impl VolumeRays {
    /// The grid of a volume's full-detail triangles, or `None` when it has none.
    pub(super) fn new(geometry: &VolumeGeometry, chunks: &[Arc<VoxelChunk>]) -> Option<Self> {
        let chunk_size = f64::from(geometry.chunk_size);
        let block = chunk_size / f64::from(BLOCKS_PER_CHUNK);
        let lo = axes(geometry.origin.to_f64()).map(|v| v - chunk_size);
        let dims = geometry
            .chunks
            .map(|c| (i32::from(c) + 2) * BLOCKS_PER_CHUNK);
        // A block id must fit a `u32`: a volume made by hand could ask for more.
        let blocks: i64 = dims.iter().map(|&d| i64::from(d)).product();
        if blocks > i64::from(u32::MAX) {
            return None;
        }
        let mut grid = Self {
            lo,
            block,
            dims,
            keys: Vec::new(),
            starts: Vec::new(),
            members: Vec::new(),
            tris: Vec::new(),
        };
        let mut pairs: Vec<(u32, u32)> = Vec::new();
        for &index in geometry.table.iter().filter(|&&i| i != NO_CHUNK) {
            let Some(chunk) = chunks.get(index as usize) else {
                continue;
            };
            let mesh = &chunk.lods[0];
            for t in &mesh.triangles {
                let corner = |i: u16| mesh.vertices.get(usize::from(i)).map(|v| v.pos);
                let (Some(a), Some(b), Some(c)) = (corner(t[0]), corner(t[1]), corner(t[2])) else {
                    continue;
                };
                let id = grid.tris.len() as u32;
                grid.tris.push([a, b, c].map(Vec3::to_f64));
                grid.add_to_blocks(&[a, b, c], id, &mut pairs);
            }
        }
        if grid.tris.is_empty() {
            return None;
        }
        pairs.sort_unstable();
        grid.group(&pairs);
        Some(grid)
    }

    fn block_range(&self, lo: f32, hi: f32, axis: usize) -> (i32, i32) {
        let at = |v: f32| {
            let rel = (f64::from(v) - self.lo[axis]) / self.block;
            (rel.floor() as i32).clamp(0, self.dims[axis] - 1)
        };
        (at(lo), at(hi))
    }

    fn add_to_blocks(&self, c: &[Vec3; 3], tri: u32, out: &mut Vec<(u32, u32)>) {
        let (lo, hi) = (c[0].min(c[1]).min(c[2]), c[0].max(c[1]).max(c[2]));
        let (x0, x1) = self.block_range(lo.x, hi.x, 0);
        let (y0, y1) = self.block_range(lo.y, hi.y, 1);
        let (z0, z1) = self.block_range(lo.z, hi.z, 2);
        for z in z0..=z1 {
            for y in y0..=y1 {
                for x in x0..=x1 {
                    out.push((self.block_id([x, y, z]), tri));
                }
            }
        }
    }

    fn block_id(&self, b: [i32; 3]) -> u32 {
        (b[0] + self.dims[0] * (b[1] + self.dims[1] * b[2])) as u32
    }

    fn group(&mut self, pairs: &[(u32, u32)]) {
        for (n, &(id, tri)) in pairs.iter().enumerate() {
            if n == 0 || pairs[n - 1].0 != id {
                self.keys.push(id);
                self.starts.push(n as u32);
            }
            self.members.push(tri);
        }
        self.starts.push(pairs.len() as u32);
    }

    /// The part of the segment `a + t d`, `t` in `0..=1`, inside the grid.
    fn clip(&self, a: Vec3d, d: Vec3d) -> Option<(f64, f64)> {
        let (a, d) = (axes(a), axes(d));
        let (mut t0, mut t1) = (0.0f64, 1.0f64);
        for k in 0..3 {
            let hi = self.lo[k] + f64::from(self.dims[k]) * self.block;
            if d[k] == 0.0 {
                if a[k] < self.lo[k] || a[k] > hi {
                    return None;
                }
                continue;
            }
            let (p, q) = ((self.lo[k] - a[k]) / d[k], (hi - a[k]) / d[k]);
            t0 = t0.max(p.min(q));
            t1 = t1.min(p.max(q));
        }
        (t0 <= t1).then_some((t0, t1))
    }

    /// Whether the segment `a`..`b` crosses no triangle of the volume.
    pub(super) fn clear(&self, a: Vec3d, b: Vec3d) -> bool {
        let d = b - a;
        let Some((t0, t1)) = self.clip(a, d) else {
            return true;
        };
        let (a_k, d_k) = (axes(a), axes(d));
        let mut cell = [0i32; 3];
        let mut step = [0i32; 3];
        let mut t_max = [f64::INFINITY; 3];
        let mut t_delta = [f64::INFINITY; 3];
        for k in 0..3 {
            let at = a_k[k] + d_k[k] * t0;
            cell[k] = (((at - self.lo[k]) / self.block).floor() as i32).clamp(0, self.dims[k] - 1);
            if d_k[k] != 0.0 {
                step[k] = if d_k[k] > 0.0 { 1 } else { -1 };
                let edge = f64::from(cell[k] + i32::from(step[k] > 0)) * self.block + self.lo[k];
                t_max[k] = t0 + (edge - at) / d_k[k];
                t_delta[k] = self.block / d_k[k].abs();
            }
        }
        // Each step crosses one block face, so the walk ends within this many.
        for _ in 0..self.dims.iter().sum::<i32>() * 2 + 3 {
            if !self.block_clear(cell, a, d) {
                return false;
            }
            let k = (0..3).fold(0, |m, k| if t_max[k] < t_max[m] { k } else { m });
            if t_max[k] > t1 {
                return true;
            }
            cell[k] += step[k];
            if cell[k] < 0 || cell[k] >= self.dims[k] {
                return true;
            }
            t_max[k] += t_delta[k];
        }
        true
    }

    fn block_clear(&self, cell: [i32; 3], a: Vec3d, d: Vec3d) -> bool {
        let Ok(n) = self.keys.binary_search(&self.block_id(cell)) else {
            return true;
        };
        let (from, to) = (self.starts[n] as usize, self.starts[n + 1] as usize);
        !self.members[from..to]
            .iter()
            .any(|&t| crosses(&self.tris[t as usize], a, d))
    }
}

/// Whether the segment `a + t d`, `t` in `0..1`, passes through the triangle.
/// Möller and Trumbore, with the ends of the segment and the edges of the
/// triangle given the slack the tolerances name.
fn crosses(tri: &[Vec3d; 3], a: Vec3d, d: Vec3d) -> bool {
    let (e1, e2) = (tri[1] - tri[0], tri[2] - tri[0]);
    let p = d.cross(e2);
    let det = e1.dot(p);
    if det.abs() < epsilon::OCCLUDER_PARALLEL {
        return false;
    }
    let inv = 1.0 / det;
    let s = a - tri[0];
    let slack = epsilon::OCCLUDER_EDGE_SLACK;
    let u = s.dot(p) * inv;
    if u < -slack || u > 1.0 + slack {
        return false;
    }
    let q = s.cross(e1);
    let v = d.dot(q) * inv;
    if v < -slack || u + v > 1.0 + slack {
        return false;
    }
    let t = e2.dot(q) * inv;
    t > epsilon::OCCLUDER_RAY_MARGIN && t < 1.0 - epsilon::OCCLUDER_RAY_MARGIN
}
