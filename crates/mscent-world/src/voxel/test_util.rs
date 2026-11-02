//! Shared helpers for the voxel tests.

use std::collections::HashMap;

use mscent_math::Vec3d;
use mscent_math::rng::Rng;

use super::{CHUNK_EDGE, ChunkMesh, Context, Mesher, Volume};
use crate::brush::Brush;

pub type Meshes = Vec<([u16; 3], ChunkMesh)>;

/// Every chunk coordinate of `volume`.
pub fn coords(volume: &Volume) -> Vec<[u16; 3]> {
    let [cx, cy, cz] = volume.chunks();
    let mut out = Vec::new();
    for z in 0..cz {
        for y in 0..cy {
            for x in 0..cx {
                out.push([x, y, z]);
            }
        }
    }
    out
}

/// Every chunk of `volume`, meshed fresh.
pub fn mesh_all(volume: &Volume, brushes: &[Brush]) -> Meshes {
    let mut ctx = Context::new(volume, brushes);
    let mut mesher = Mesher::new();
    coords(volume)
        .into_iter()
        .map(|c| (c, mesher.mesh(volume, &mut ctx, c).expect("chunk exists")))
        .collect()
}

/// The cell a vertex stands for, across the whole volume.
pub fn global_cell(coord: [u16; 3], cell: [i8; 3]) -> [i32; 3] {
    [0, 1, 2].map(|a| i32::from(coord[a]) * CHUNK_EDGE as i32 + i32::from(cell[a]))
}

/// Edges with no matching opposite edge: holes in the surface. A closed
/// surface has none, even where it pinches and four triangles share an edge.
pub fn open_edges(meshes: &Meshes) -> Vec<([i32; 3], [i32; 3])> {
    let mut edges: HashMap<([i32; 3], [i32; 3]), i32> = HashMap::new();
    for (coord, mesh) in meshes {
        let key = |i: u16| global_cell(*coord, mesh.vertices[usize::from(i)].cell);
        for t in &mesh.triangles {
            for k in 0..3 {
                *edges.entry((key(t[k]), key(t[(k + 1) % 3]))).or_insert(0) += 1;
            }
        }
    }
    edges
        .iter()
        .filter(|&(&(a, b), &n)| edges.get(&(b, a)).copied().unwrap_or(0) != n)
        .map(|(&e, _)| e)
        .collect()
}

/// Asserts that two chunks using the same cell put its vertex in the same place.
pub fn assert_shared_vertices_agree(meshes: &Meshes) {
    let mut seen = HashMap::new();
    for (coord, mesh) in meshes {
        for v in &mesh.vertices {
            let g = global_cell(*coord, v.cell);
            // Merged interior vertices belong to one chunk, so only a cell on
            // a chunk's border can repeat here.
            let (p, n) = *seen.entry(g).or_insert((v.pos, v.normal));
            assert_eq!(p, v.pos, "position of cell {g:?}");
            assert_eq!(n, v.normal, "normal of cell {g:?}");
        }
    }
}

/// A warped gyroid, positive solid: the kind of open shape caves come from.
pub fn gyroid(p: Vec3d) -> f64 {
    let g = |x: f64, y: f64, z: f64| x.sin() * y.cos() + y.sin() * z.cos() + z.sin() * x.cos();
    g(p.x * 0.55, p.y * 0.55, p.z * 0.55)
        + g(p.x * 1.7 + 3.0, p.y * 1.7, p.z * 1.7 + 1.0) * 0.18
        + 0.25
}

/// A random point in the box of `volume`.
pub fn random_point(rng: &mut Rng, volume: &Volume) -> Vec3d {
    let (lo, hi) = volume.bounds();
    Vec3d::new(
        rng.range_f64(lo.x, hi.x),
        rng.range_f64(lo.y, hi.y),
        rng.range_f64(lo.z, hi.z),
    )
}
