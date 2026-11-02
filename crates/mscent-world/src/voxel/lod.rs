//! Level of detail by vertex clustering. Interior cells merge into blocks;
//! cells on the border of a chunk's mesh never merge. Those are the only
//! vertices a neighbouring chunk shares, so meshes of any two levels still meet
//! exactly, and no stitching exists to go wrong.

use mscent_math::{Aabb, Vec3};

use super::CHUNK_EDGE;
use super::surface::{CELLS, ChunkMesh, VoxelVertex, cell_index};

/// A reduced level of a chunk's mesh.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lod {
    /// Interior cells merge in blocks of 2 per axis.
    Half,
    /// Interior cells merge in blocks of 4 per axis.
    Quarter,
}

impl Lod {
    const fn block(self) -> i32 {
        match self {
            Lod::Half => 2,
            Lod::Quarter => 4,
        }
    }
}

/// True for a cell a neighbouring chunk's mesh also uses.
fn on_border(cell: [i8; 3]) -> bool {
    cell.iter()
        .any(|&c| c < 0 || i32::from(c) >= CHUNK_EDGE as i32 - 1)
}

/// A merged vertex while it collects its members.
#[derive(Clone, Copy)]
struct Merged {
    pos: Vec3,
    normal: Vec3,
    cell: [i8; 3],
    members: f32,
}

impl ChunkMesh {
    /// This mesh at a lower level of detail. Triangles that collapse are gone;
    /// the air mask stays, since air has not changed.
    pub fn clustered(&self, lod: Lod) -> ChunkMesh {
        let (merged, remap) = merge_vertices(&self.vertices, lod);
        let vertices: Vec<VoxelVertex> = merged.iter().map(finish_vertex).collect();
        let mut triangles = Vec::with_capacity(self.triangles.len());
        let mut materials = Vec::with_capacity(self.materials.len());
        for (t, &m) in self.triangles.iter().zip(&self.materials) {
            let [a, b, c] = t.map(|i| remap[usize::from(i)]);
            if a != b && b != c && a != c {
                triangles.push([a, b, c]);
                materials.push(m);
            }
        }
        ChunkMesh {
            bounds: vertices
                .iter()
                .fold(Aabb::empty(), |b, v| b.including(v.pos)),
            vertices,
            triangles,
            materials,
            air: self.air,
        }
    }
}

/// The merged vertices, and the new index of each old vertex.
fn merge_vertices(vertices: &[VoxelVertex], lod: Lod) -> (Vec<Merged>, Vec<u16>) {
    let size = lod.block();
    let mut slots = vec![u16::MAX; CELLS];
    let mut merged: Vec<Merged> = Vec::new();
    let mut remap = Vec::with_capacity(vertices.len());
    for v in vertices {
        let key = if on_border(v.cell) {
            v.cell.map(i32::from)
        } else {
            v.cell.map(|c| i32::from(c) / size * size)
        };
        let slot = &mut slots[cell_index(key)];
        if *slot == u16::MAX {
            *slot = merged.len() as u16;
            merged.push(Merged {
                pos: Vec3::ZERO,
                normal: Vec3::ZERO,
                cell: v.cell,
                members: 0.0,
            });
        }
        let m = &mut merged[usize::from(*slot)];
        m.pos = m.pos + v.pos;
        m.normal = m.normal + v.normal;
        m.members += 1.0;
        remap.push(*slot);
    }
    (merged, remap)
}

fn finish_vertex(m: &Merged) -> VoxelVertex {
    // A lone vertex keeps its exact bits: a border vertex that moved or changed
    // its normal would show as a crack or a shading seam against its neighbour.
    if m.members == 1.0 {
        return VoxelVertex {
            pos: m.pos,
            normal: m.normal,
            cell: m.cell,
        };
    }
    VoxelVertex {
        pos: m.pos * (1.0 / m.members),
        normal: m.normal.normalized().unwrap_or(Vec3::new(0.0, 0.0, 1.0)),
        cell: m.cell,
    }
}
