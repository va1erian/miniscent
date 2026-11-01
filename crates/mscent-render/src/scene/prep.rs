//! Per-level data derived once at load: face polygons with texture
//! coordinates, per-leaf face lists and per-node bounding boxes.

use mscent_math::{Aabb, Polygon, Vec3};
use mscent_world::{Bsp, Child, Face};

use crate::raster::Vertex;

/// Texture size assumed for a material the table does not hold, so that
/// the default mapping still tiles every 8 world units.
const FALLBACK_TEXTURE_SIZE: (u32, u32) = (128, 128);

/// A face as the rasterizer wants it: texture coordinates in repeats from
/// the face's [`TexInfo`](mscent_world::TexInfo) and the texture's size.
pub(super) fn face_vertices(face: &Face, texture_size: Option<(u32, u32)>) -> Vec<Vertex> {
    let size = texture_size.unwrap_or(FALLBACK_TEXTURE_SIZE);
    face.polygon
        .vertices()
        .iter()
        .map(|&p| {
            let (u, v) = face.texinfo.uv_at(p, size);
            Vertex::new(p.to_f32(), u, v)
        })
        .collect()
}

pub(super) fn polygon_points(poly: &Polygon) -> Vec<Vec3> {
    poly.vertices().iter().map(|v| v.to_f32()).collect()
}

/// For every leaf, the indices of the faces that border it.
pub(super) fn leaf_faces(leaf_count: usize, faces: &[Face]) -> Vec<Vec<u32>> {
    let mut out = vec![Vec::new(); leaf_count];
    for (i, face) in (0u32..).zip(faces) {
        for leaf in &face.front_leaves {
            if let Some(list) = out.get_mut(leaf.0 as usize) {
                list.push(i);
            }
        }
    }
    out
}

/// Bounding box of each leaf's faces.
pub(super) fn leaf_boxes(per_leaf: &[Vec<u32>], polys: &[Vec<Vertex>]) -> Vec<Aabb> {
    per_leaf
        .iter()
        .map(|list| {
            list.iter()
                .filter_map(|&i| polys.get(i as usize))
                .flatten()
                .fold(Aabb::empty(), |b, v| b.including(v.pos))
        })
        .collect()
}

/// Bounding box of every node's subtree, indexed by node id. Boxes cover the
/// faces of the leaves below; a subtree with no faces has an empty box.
pub(super) fn node_bounds(bsp: &Bsp, leaf_boxes: &[Aabb]) -> Vec<Aabb> {
    let mut out = vec![Aabb::empty(); bsp.nodes().len()];
    subtree(bsp, bsp.root(), leaf_boxes, &mut out);
    out
}

fn subtree(bsp: &Bsp, child: Child, leaf_boxes: &[Aabb], out: &mut [Aabb]) -> Aabb {
    match child {
        Child::Leaf(id) => leaf_boxes
            .get(id.0 as usize)
            .copied()
            .unwrap_or_else(Aabb::empty),
        Child::Node(id) => {
            let Some(node) = bsp.node(id) else {
                return Aabb::empty();
            };
            // Depth is bounded by `Bsp::from_parts`, so recursion is shallow.
            let b = subtree(bsp, node.front, leaf_boxes, out)
                .union(subtree(bsp, node.back, leaf_boxes, out));
            if let Some(slot) = out.get_mut(id.0 as usize) {
                *slot = b;
            }
            b
        }
    }
}
