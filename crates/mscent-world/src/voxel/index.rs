//! A per-cell index over one chunk's full-detail triangles, so a query for a
//! box visits only the triangles in the cells the box touches.
//!
//! The grid is [`INDEX_EDGE`] cells along each axis, laid over the mesh's own
//! bounds rather than the chunk's nominal box. Vertices of a surface-nets mesh
//! may sit a cell outside the chunk, and bounds that describe the mesh itself
//! need no such assumption.
//!
//! Memory is bounded: a table of `INDEX_EDGE^3 + 1` offsets (2 KiB) and at most
//! [`MAX_ENTRIES_PER_TRIANGLE`] entries of 4 bytes per triangle. A mesh that
//! would need more (nothing the mesher makes, but the index does not take that
//! on trust) gets no index, and every query then scans all its triangles.

use mscent_math::Vec3;

use super::surface::{ChunkMesh, VoxelVertex};

/// Cells along one axis of the grid.
pub const INDEX_EDGE: usize = 8;
const CELLS: usize = INDEX_EDGE * INDEX_EDGE * INDEX_EDGE;
/// Most cell entries per triangle, on average, before the index is given up.
const MAX_ENTRIES_PER_TRIANGLE: usize = 16;
/// An entry is the triangle's number above the cell it first appears in, three
/// bits an axis, so a query can tell whether a cell is the one to report it
/// from without reading the triangle.
const OWNER_BITS: u32 = 9;
const _: () = assert!(INDEX_EDGE == 8, "three bits an axis");
/// Meshes with more triangles than an entry can name get no index.
const MAX_INDEXED_TRIANGLES: usize = 1 << (32 - OWNER_BITS);

/// Triangles of a mesh by grid cell. See the module docs.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CellIndex {
    lo: [f32; 3],
    /// Cells per world unit on each axis; zero where the mesh is flat.
    inv: [f32; 3],
    /// `CELLS + 1` offsets into `entries`, or empty when there is no index.
    starts: Vec<u32>,
    /// Triangle number and first cell, see [`OWNER_BITS`].
    entries: Vec<u32>,
    triangles: u32,
}

/// The vertices of triangle `t` of `mesh`, or `None` if it names a vertex the
/// mesh does not have.
pub(super) fn triangle(mesh: &ChunkMesh, t: usize) -> Option<[VoxelVertex; 3]> {
    let tri = mesh.triangles.get(t)?;
    let at = |i: u16| mesh.vertices.get(usize::from(i)).copied();
    Some([at(tri[0])?, at(tri[1])?, at(tri[2])?])
}

impl CellIndex {
    /// Indexes `mesh`. Triangles with an out of range vertex are left out.
    pub fn build(mesh: &ChunkMesh) -> Self {
        let triangles = u32::try_from(mesh.triangles.len()).unwrap_or(u32::MAX);
        if mesh.triangles.len() >= MAX_INDEXED_TRIANGLES {
            return Self::unindexed(triangles);
        }
        let mut index = Self::grid(mesh);
        index.triangles = triangles;
        let limit = mesh
            .triangles
            .len()
            .saturating_mul(MAX_ENTRIES_PER_TRIANGLE);
        let mut counts = vec![0u32; CELLS + 1];
        let mut total = 0usize;
        for t in 0..mesh.triangles.len() {
            let Some(c) = triangle(mesh, t) else { continue };
            let (first, last) = index.cell_range(&c);
            total = total.saturating_add(span(first, last));
            if total > limit {
                return Self::unindexed(triangles);
            }
            for_cells(first, last, |cell| counts[cell + 1] += 1);
        }
        for i in 0..CELLS {
            counts[i + 1] += counts[i];
        }
        let mut entries = vec![0u32; total];
        let mut next = counts.clone();
        for t in 0..mesh.triangles.len() {
            let Some(c) = triangle(mesh, t) else { continue };
            let (first, last) = index.cell_range(&c);
            let entry = (t as u32) << OWNER_BITS | pack(first);
            for_cells(first, last, |cell| {
                entries[next[cell] as usize] = entry;
                next[cell] += 1;
            });
        }
        index.starts = counts;
        index.entries = entries;
        index
    }

    fn unindexed(triangles: u32) -> Self {
        Self {
            triangles,
            ..Self::default()
        }
    }

    /// Cell size from the mesh bounds. An empty or non-finite mesh gets a grid
    /// whose cells all collapse to the first.
    fn grid(mesh: &ChunkMesh) -> Self {
        let (lo, hi) = (mesh.bounds.min, mesh.bounds.max);
        let (lo, hi) = ([lo.x, lo.y, lo.z], [hi.x, hi.y, hi.z]);
        let mut index = Self::default();
        for a in 0..3 {
            let extent = hi[a] - lo[a];
            let ok = extent.is_finite() && extent > 0.0 && lo[a].is_finite();
            index.lo[a] = if ok { lo[a] } else { 0.0 };
            index.inv[a] = if ok { INDEX_EDGE as f32 / extent } else { 0.0 };
        }
        index
    }

    fn coord(&self, axis: usize, v: f32) -> usize {
        // NaN converts to zero, so a bad coordinate lands in the first cell.
        let c = ((v - self.lo[axis]) * self.inv[axis]).floor() as usize;
        c.min(INDEX_EDGE - 1)
    }

    fn cell_range(&self, c: &[VoxelVertex; 3]) -> ([usize; 3], [usize; 3]) {
        let mut first = [0; 3];
        let mut last = [0; 3];
        for a in 0..3 {
            let v = c.map(|p| [p.pos.x, p.pos.y, p.pos.z][a]);
            let (min, max) = (v[0].min(v[1]).min(v[2]), v[0].max(v[1]).max(v[2]));
            first[a] = self.coord(a, min);
            last[a] = self.coord(a, max);
        }
        (first, last)
    }

    /// Calls `f` with each triangle of `mesh` whose cells meet
    /// the box `lo..=hi`, once each. `mesh` must be the one this was built from.
    pub(super) fn query(
        &self,
        mesh: &ChunkMesh,
        lo: Vec3,
        hi: Vec3,
        mut f: impl FnMut([VoxelVertex; 3]),
    ) {
        if self.starts.is_empty() {
            let all = 0..mesh.triangles.len();
            all.filter_map(|t| triangle(mesh, t)).for_each(f);
            return;
        }
        let first = [0, 1, 2].map(|a| self.coord(a, [lo.x, lo.y, lo.z][a]));
        let last = [0, 1, 2].map(|a| self.coord(a, [hi.x, hi.y, hi.z][a]));
        for_cells(first, last, |cell| {
            let (from, to) = (self.starts[cell] as usize, self.starts[cell + 1] as usize);
            let here = cell_coords(cell);
            for &entry in self.entries.get(from..to).unwrap_or_default() {
                // A triangle lies in several cells. It is handed over from the
                // first of them the box covers, so no triangle is tested twice.
                let born = unpack(entry);
                if [0, 1, 2].map(|a| born[a].max(first[a])) != here {
                    continue;
                }
                if let Some(c) = triangle(mesh, (entry >> OWNER_BITS) as usize) {
                    f(c);
                }
            }
        });
    }

    /// Walks outward from `p` ring by ring of cells, handing `visit` each
    /// triangle of a cell that could hold something closer than `bound`, the
    /// best squared distance so far, which `visit` lowers as it finds closer
    /// ones. Stops once no further ring can beat `bound`.
    pub(super) fn nearest(
        &self,
        mesh: &ChunkMesh,
        p: [f64; 3],
        bound: &mut f64,
        mut visit: impl FnMut([VoxelVertex; 3], &mut f64),
    ) {
        if self.starts.is_empty() {
            for tri in (0..mesh.triangles.len()).filter_map(|t| triangle(mesh, t)) {
                visit(tri, bound);
            }
            return;
        }
        let gaps = [0, 1, 2].map(|a| self.axis_gaps(a, p[a]));
        let home = [0, 1, 2].map(|a| self.coord(a, p[a] as f32));
        // Ring `r` can be as near as `r - 1` steps: `p` may lie at the very edge
        // of its own cell, right beside the next.
        let step = (0..3)
            .filter(|&a| self.inv[a] > 0.0)
            .map(|a| f64::from(1.0 / self.inv[a]))
            .fold(f64::INFINITY, f64::min);
        for ring in 0..INDEX_EDGE {
            if *bound <= (ring.saturating_sub(1) as f64 * step).powi(2) {
                return;
            }
            for cell in ring_cells(home, ring) {
                let c = cell_coords(cell);
                if gaps[0][c[0]] + gaps[1][c[1]] + gaps[2][c[2]] < *bound {
                    self.visit_cell(mesh, cell, bound, &mut visit);
                }
            }
        }
    }

    fn visit_cell(
        &self,
        mesh: &ChunkMesh,
        cell: usize,
        bound: &mut f64,
        visit: &mut impl FnMut([VoxelVertex; 3], &mut f64),
    ) {
        let range = self.starts[cell] as usize..self.starts[cell + 1] as usize;
        for &entry in self.entries.get(range).unwrap_or_default() {
            if let Some(tri) = triangle(mesh, (entry >> OWNER_BITS) as usize) {
                visit(tri, bound);
            }
        }
    }

    /// Squared distance from coordinate `v` to each cell's slab of `axis`. A
    /// flat axis has slabs of no thickness at the mesh's own plane.
    fn axis_gaps(&self, axis: usize, v: f64) -> [f64; INDEX_EDGE] {
        let size = if self.inv[axis] > 0.0 {
            f64::from(1.0 / self.inv[axis])
        } else {
            0.0
        };
        std::array::from_fn(|c| {
            let lo = f64::from(self.lo[axis]) + size * c as f64;
            let gap = (lo - v).max(v - (lo + size)).max(0.0);
            gap * gap
        })
    }

    /// Triangles indexed, for tests and memory accounting.
    pub fn triangles(&self) -> u32 {
        self.triangles
    }

    /// Bytes the index holds.
    pub fn bytes(&self) -> usize {
        (self.starts.len() + self.entries.len()) * std::mem::size_of::<u32>()
    }
}

const fn cell_id(c: [usize; 3]) -> usize {
    c[0] + c[1] * INDEX_EDGE + c[2] * INDEX_EDGE * INDEX_EDGE
}

fn pack(c: [usize; 3]) -> u32 {
    (c[0] | c[1] << 3 | c[2] << 6) as u32
}

fn unpack(entry: u32) -> [usize; 3] {
    let e = entry as usize;
    [e & 7, e >> 3 & 7, e >> 6 & 7]
}

fn cell_coords(cell: usize) -> [usize; 3] {
    [
        cell % INDEX_EDGE,
        (cell / INDEX_EDGE) % INDEX_EDGE,
        cell / (INDEX_EDGE * INDEX_EDGE),
    ]
}

/// The cells at Chebyshev distance exactly `ring` from `home`, inside the grid.
fn ring_cells(home: [usize; 3], ring: usize) -> impl Iterator<Item = usize> {
    let edge = INDEX_EDGE as isize;
    let r = ring as isize;
    let h = home.map(|v| v as isize);
    (-r..=r).flat_map(move |dz| {
        (-r..=r).flat_map(move |dy| {
            (-r..=r).filter_map(move |dx| {
                let on_ring = dx.abs().max(dy.abs()).max(dz.abs()) == r;
                let c = [h[0] + dx, h[1] + dy, h[2] + dz];
                let inside = c.iter().all(|&v| (0..edge).contains(&v));
                (on_ring && inside).then(|| cell_id(c.map(|v| v as usize)))
            })
        })
    })
}

fn span(first: [usize; 3], last: [usize; 3]) -> usize {
    (0..3).map(|a| last[a] - first[a] + 1).product()
}

fn for_cells(first: [usize; 3], last: [usize; 3], mut f: impl FnMut(usize)) {
    for z in first[2]..=last[2] {
        for y in first[1]..=last[1] {
            for x in first[0]..=last[0] {
                f(cell_id([x, y, z]));
            }
        }
    }
}
