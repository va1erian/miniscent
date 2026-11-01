//! Turns empty leaf cells into surface fragments.

use crate::bsp::{Bsp, Child, LeafId, LeafKind};
use crate::{csg, polytope};
use mscent_math::{DPlane, Polygon, Split};

/// A piece of surface between one empty leaf and one solid leaf.
pub(super) struct Fragment {
    /// Wound counter-clockwise seen from the empty side.
    pub polygon: Polygon,
    /// Normal points into the empty side.
    pub plane: DPlane,
    pub empty: LeafId,
    pub solid: LeafId,
}

pub(super) fn fragments(bsp: &Bsp) -> Vec<Fragment> {
    let mut out = Vec::new();
    walk(bsp, bsp.root(), &mut Vec::new(), &mut out);
    out
}

/// Visits leaves keeping `cell`: outward planes bounding the current region.
fn walk(bsp: &Bsp, child: Child, cell: &mut Vec<DPlane>, out: &mut Vec<Fragment>) {
    match child {
        Child::Leaf(id) => {
            if bsp.leaf_kind(id) == LeafKind::Empty {
                leaf_faces(bsp, id, cell, out);
            }
        }
        Child::Node(id) => {
            let Some(node) = bsp.node(id) else { return };
            // Outward form of the front half-space is the flipped plane.
            cell.push(node.plane.flipped());
            walk(bsp, node.front, cell, out);
            cell.pop();
            cell.push(node.plane);
            walk(bsp, node.back, cell, out);
            cell.pop();
        }
    }
}

fn leaf_faces(bsp: &Bsp, leaf: LeafId, cell: &[DPlane], out: &mut Vec<Fragment>) {
    // The world box closes cells the tree leaves open; its faces are skipped.
    let mut planes = cell.to_vec();
    planes.extend(csg::world_box());
    for (i, poly) in polytope::faces(&planes) {
        if i < cell.len() {
            push_through(bsp, bsp.root(), poly, &planes[i], leaf, out);
        }
    }
}

/// Sends `poly` (on `outward`, which points away from the empty leaf) down
/// the tree and keeps the pieces that end in solid leaves.
fn push_through(
    bsp: &Bsp,
    child: Child,
    poly: Polygon,
    outward: &DPlane,
    empty: LeafId,
    out: &mut Vec<Fragment>,
) {
    match child {
        Child::Leaf(solid) => {
            if bsp.leaf_kind(solid) == LeafKind::Solid {
                out.push(Fragment {
                    polygon: reversed(&poly),
                    plane: outward.flipped(),
                    empty,
                    solid,
                });
            }
        }
        Child::Node(id) => {
            let Some(node) = bsp.node(id) else { return };
            match poly.split_by_plane(&node.plane) {
                Split::Front => push_through(bsp, node.front, poly, outward, empty, out),
                Split::Back => push_through(bsp, node.back, poly, outward, empty, out),
                // Coplanar with the node: the far side is wherever the
                // outward normal points, never the empty leaf we came from.
                Split::On => {
                    let next = if outward.normal.dot(node.plane.normal) > 0.0 {
                        node.front
                    } else {
                        node.back
                    };
                    push_through(bsp, next, poly, outward, empty, out);
                }
                Split::Spanning { front, back } => {
                    push_through(bsp, node.front, front, outward, empty, out);
                    push_through(bsp, node.back, back, outward, empty, out);
                }
            }
        }
    }
}

fn reversed(poly: &Polygon) -> Polygon {
    let mut v = poly.vertices().to_vec();
    v.reverse();
    // Same vertex count and finiteness as the input, so this cannot fail;
    // fall back to the original rather than panic.
    Polygon::new(v).unwrap_or_else(|| poly.clone())
}
