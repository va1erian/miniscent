//! Surface nets: a vertex in each cell the surface crosses, a quad across each
//! sample edge it crosses.
//!
//! A cell is named by its lowest sample, `-2..=16` per axis in a chunk's local
//! coordinates, so cell 15 of a chunk is cell -1 of the next. A chunk owns the
//! quads of the edges whose lower sample it holds. Border vertices are built
//! from the same samples in both chunks, so neighbouring meshes meet exactly.
//!
//! The chunks at the border of a volume also own the edges in its ring, so the
//! surface continues past the box on every side, behind the brush faces it
//! meets there. That overlap is what leaves no gap between the two.

use mscent_math::epsilon::{VOXEL_NORMAL_MIN_AGREEMENT, VOXEL_NORMAL_SAME};
use mscent_math::{Aabb, Vec3, Vec3d};

use super::CHUNK_EDGE;
use super::block::{BLOCK_EDGE, Block, block_index, fill_block};
use super::connectivity::{AirMask, air_mask};
use super::context::Context;
use super::volume::Volume;

/// Cells along one edge of a chunk's mesh.
const CELL_EDGE: usize = CHUNK_EDGE + 3;
pub(super) const CELLS: usize = CELL_EDGE * CELL_EDGE * CELL_EDGE;
const NO_VERTEX: u16 = u16::MAX;

/// Sample edges of a cell, as corner pairs. A corner's bits are its offsets.
const CELL_EDGES: [(usize, usize); 12] = [
    (0, 1),
    (2, 3),
    (4, 5),
    (6, 7),
    (0, 2),
    (1, 3),
    (4, 6),
    (5, 7),
    (0, 4),
    (1, 5),
    (2, 6),
    (3, 7),
];

pub(super) const fn cell_index(c: [i32; 3]) -> usize {
    let e = CELL_EDGE as i32;
    ((c[0] + 2) + (c[1] + 2) * e + (c[2] + 2) * e * e) as usize
}

/// Weights along an axis of the smoothed normal: a slope along it, and a blur
/// across it. Both are a cell's own difference spread over its neighbours.
const SLOPE: [f32; 4] = [-1.0, -1.0, 1.0, 1.0];
const BLUR: [f32; 4] = [1.0, 3.0, 3.0, 1.0];

/// One vertex of a chunk's surface.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VoxelVertex {
    pub pos: Vec3,
    /// Unit length, pointing into air.
    pub normal: Vec3,
    /// The cell it stands for, in the chunk's local coordinates.
    pub cell: [i8; 3],
}

/// The surface in one chunk. Triangles wind counter-clockwise from the air
/// side and are parallel to `materials`.
#[derive(Clone, Debug, PartialEq)]
pub struct ChunkMesh {
    pub vertices: Vec<VoxelVertex>,
    pub triangles: Vec<[u16; 3]>,
    /// Material id of each triangle.
    pub materials: Vec<u8>,
    pub bounds: Aabb,
    pub air: AirMask,
}

/// Holds the scratch space of meshing, so a run over many chunks reuses it.
pub struct Mesher {
    block: Box<Block>,
    cells: Box<[u16; CELLS]>,
}

impl Default for Mesher {
    fn default() -> Self {
        Self::new()
    }
}

impl Mesher {
    pub fn new() -> Self {
        Self {
            block: Block::new(),
            cells: Box::new([NO_VERTEX; CELLS]),
        }
    }

    /// The surface of chunk `coord`, or `None` for a chunk the volume does not
    /// have, or a context built for another volume.
    pub fn mesh(
        &mut self,
        volume: &Volume,
        ctx: &mut Context,
        coord: [u16; 3],
    ) -> Option<ChunkMesh> {
        let c = coord.map(i32::from);
        if ctx.lattice != volume.lattice {
            return None;
        }
        ctx.lattice.chunk_index(c)?;
        fill_block(volume, ctx, c, &mut self.block);
        Some(self.extract(volume, c))
    }

    /// Whether the last chunk meshed has solid at `local`, a sample of its
    /// block (each coordinate `-2..=17`); anything else is not solid.
    pub(super) fn solid_at(&self, local: [i32; 3]) -> bool {
        let inside = local
            .iter()
            .all(|c| (-2..CHUNK_EDGE as i32 + 2).contains(c));
        inside && self.block.density[block_index(local[0], local[1], local[2])] >= 0
    }

    /// The resolved samples of the last chunk meshed.
    #[cfg(test)]
    pub(super) fn block(&self) -> &Block {
        &self.block
    }

    fn extract(&mut self, volume: &Volume, coord: [i32; 3]) -> ChunkMesh {
        let all_air = self.block.density.iter().all(|&d| d < 0);
        let all_solid = self.block.density.iter().all(|&d| d >= 0);
        let mut build = Build {
            block: &self.block,
            cells: &mut self.cells,
            volume,
            first: coord.map(|c| c * CHUNK_EDGE as i32),
            vertices: Vec::new(),
            triangles: Vec::new(),
            materials: Vec::new(),
        };
        build.cells.fill(NO_VERTEX);
        if !all_air && !all_solid {
            let chunks = volume.lattice.chunks.map(i32::from);
            let edge = CHUNK_EDGE as i32;
            for axis in 0..3 {
                // Only chunks at the border own edges in the ring. Rows of
                // quads across an axis need a cell on each side of the row, so
                // they start one sample later than edges along it.
                let (first, last) = (
                    coord.map(|c| c == 0),
                    [0, 1, 2].map(|a| coord[a] == chunks[a] - 1),
                );
                let end = last.map(|l| if l { edge + 1 } else { edge });
                let across = first.map(|f| if f { -1 } else { 0 });
                let along = if first[axis] { -2 } else { 0 }..end[axis];
                build.quads_along(axis, along, across, end);
            }
        }
        let bounds = build
            .vertices
            .iter()
            .fold(Aabb::empty(), |b, v| b.including(v.pos));
        ChunkMesh {
            air: match (all_air, all_solid) {
                (true, _) => AirMask::ALL,
                (_, true) => AirMask::NONE,
                _ => air_mask(&self.block),
            },
            vertices: build.vertices,
            triangles: build.triangles,
            materials: build.materials,
            bounds,
        }
    }
}

// Every vertex index fits, with the empty mark to spare.
const _: () = assert!(CELLS < NO_VERTEX as usize);

struct Build<'a> {
    block: &'a Block,
    cells: &'a mut [u16; CELLS],
    volume: &'a Volume,
    first: [i32; 3],
    vertices: Vec<VoxelVertex>,
    triangles: Vec<[u16; 3]>,
    materials: Vec<u8>,
}

impl Build<'_> {
    /// The quads of the edges along `axis` with a lower sample in `along`, in
    /// the rows `across..end` of the other two axes.
    fn quads_along(
        &mut self,
        axis: usize,
        along: std::ops::Range<i32>,
        across: [i32; 3],
        end: [i32; 3],
    ) {
        // Cyclic order, so that `u x v` is `+axis`.
        let (u, v) = ((axis + 1) % 3, (axis + 2) % 3);
        let mut step = [0; 3];
        step[axis] = 1;
        let stride = block_index(step[0], step[1], step[2]) - block_index(0, 0, 0);
        for a in along {
            for vv in across[v]..end[v] {
                for uu in across[u]..end[u] {
                    let mut s = [0; 3];
                    s[axis] = a;
                    s[u] = uu;
                    s[v] = vv;
                    let i = block_index(s[0], s[1], s[2]);
                    let (d0, d1) = (self.block.density[i], self.block.density[i + stride]);
                    if (d0 >= 0) != (d1 >= 0) {
                        let solid = if d0 >= 0 { i } else { i + stride };
                        self.quad(s, u, v, d0 >= 0, self.block.material[solid]);
                    }
                }
            }
        }
    }

    /// The quad around the edge from sample `s`. Its four cells share the edge.
    fn quad(&mut self, s: [i32; 3], u: usize, v: usize, lower_solid: bool, material: u8) {
        let cell = |du: i32, dv: i32| {
            let mut c = s;
            c[u] += du;
            c[v] += dv;
            c
        };
        let ring = [cell(-1, -1), cell(0, -1), cell(0, 0), cell(-1, 0)].map(|c| self.vertex(c));
        // With the lower sample solid the normal points along +axis, which is
        // the winding of this ring.
        let [a, b, c, d] = if lower_solid {
            ring
        } else {
            [ring[3], ring[2], ring[1], ring[0]]
        };
        // The shorter diagonal gives the better pair of triangles.
        let tris = if self.dist2(a, c) <= self.dist2(b, d) {
            [[a, b, c], [a, c, d]]
        } else {
            [[b, c, d], [b, d, a]]
        };
        self.triangles.extend(tris);
        self.materials.extend([material; 2]);
    }

    fn dist2(&self, a: u16, b: u16) -> f32 {
        let d = self.vertices[usize::from(a)].pos - self.vertices[usize::from(b)].pos;
        d.dot(d)
    }

    fn vertex(&mut self, c: [i32; 3]) -> u16 {
        let slot = cell_index(c);
        if self.cells[slot] != NO_VERTEX {
            return self.cells[slot];
        }
        let index = self.vertices.len() as u16;
        self.cells[slot] = index;
        let v = self.make_vertex(c);
        self.vertices.push(v);
        index
    }

    fn make_vertex(&self, c: [i32; 3]) -> VoxelVertex {
        let corners: [f32; 8] = std::array::from_fn(|i| {
            let k = block_index(
                c[0] + (i & 1) as i32,
                c[1] + ((i >> 1) & 1) as i32,
                c[2] + ((i >> 2) & 1) as i32,
            );
            f32::from(self.block.density[k])
        });
        VoxelVertex {
            pos: self.position(c, &corners),
            normal: self.smooth_normal(c, cell_normal(&corners)),
            cell: c.map(|v| v as i8),
        }
    }

    /// The normal read over a cell and its neighbours. One cell's gradient turns
    /// sharply wherever the grid crosses a crease, and light baked from it then
    /// draws the triangulation around every hole. Samples past the padding repeat
    /// the last, so both chunks that hold a cell read the same ones. A sheet about
    /// as thin as this reach has its two sides cancel, and keeps `narrow`. So does
    /// a surface with no crease to smooth: a flat wall's reach runs into the ring,
    /// where the surface slides behind the brush it meets, and it must not tilt.
    fn smooth_normal(&self, c: [i32; 3], narrow: Vec3) -> Vec3 {
        let sample = |x: i32, y: i32, z: i32| {
            let at = |v: i32, axis: usize| (c[axis] + v - 1).clamp(-2, BLOCK_EDGE as i32 - 3);
            f32::from(self.block.density[block_index(at(x, 0), at(y, 1), at(z, 2))])
        };
        let mut gradient = Vec3::ZERO;
        for z in 0..4 {
            for y in 0..4 {
                for x in 0..4 {
                    let d = sample(x as i32, y as i32, z as i32);
                    gradient.x += d * SLOPE[x] * BLUR[y] * BLUR[z];
                    gradient.y += d * BLUR[x] * SLOPE[y] * BLUR[z];
                    gradient.z += d * BLUR[x] * BLUR[y] * SLOPE[z];
                }
            }
        }
        (-gradient)
            .normalized()
            .filter(|wide| {
                (VOXEL_NORMAL_MIN_AGREEMENT..VOXEL_NORMAL_SAME).contains(&wide.dot(narrow))
            })
            .unwrap_or(narrow)
    }

    /// World position: the mean of the crossings on the cell's edges. It goes
    /// through `f64` from whole-cell values, so both chunks that hold this cell
    /// get the same bits.
    fn position(&self, c: [i32; 3], corners: &[f32; 8]) -> Vec3 {
        let mut sum = [0.0f64; 3];
        let mut crossings = 0.0f64;
        for &(a, b) in &CELL_EDGES {
            if (corners[a] >= 0.0) == (corners[b] >= 0.0) {
                continue;
            }
            let t = f64::from(corners[a]) / f64::from(corners[a] - corners[b]);
            for (k, s) in sum.iter_mut().enumerate() {
                let (pa, pb) = (((a >> k) & 1) as f64, ((b >> k) & 1) as f64);
                *s += pa + (pb - pa) * t;
            }
            crossings += 1.0;
        }
        let count = crossings.max(1.0);
        let lattice = &self.volume.lattice;
        let cell = lattice.cell.units();
        let at = |k: usize| {
            let whole = f64::from(lattice.origin[k]) + f64::from(self.first[k] + c[k]);
            (whole + sum[k] / count) * cell
        };
        Vec3d::new(at(0), at(1), at(2)).to_f32()
    }
}

/// The field falls toward air, so the normal is its negative gradient over the
/// cell's corners. A cell with no gradient (only possible in a saddle) gets +Z.
fn cell_normal(c: &[f32; 8]) -> Vec3 {
    let diff = |bit: usize| {
        (0..8).fold(0.0f32, |sum, i| {
            if i & (1 << bit) != 0 {
                sum + c[i]
            } else {
                sum - c[i]
            }
        })
    };
    Vec3::new(-diff(0), -diff(1), -diff(2))
        .normalized()
        .unwrap_or(Vec3::new(0.0, 0.0, 1.0))
}
