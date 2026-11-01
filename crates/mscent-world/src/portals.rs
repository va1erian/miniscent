//! Portals: the doorways between touching empty leaves.
//!
//! Every BSP node splits space with a plane. The part of that plane that lies
//! inside the node's own cell is pushed down the front subtree, then each
//! resulting fragment down the back subtree; where an empty leaf meets an
//! empty leaf, the fragment is a portal. Leaves that only touch along an edge
//! or a corner have no area in common and get no portal, which is what a
//! visibility or flood-fill pass wants.

use crate::bsp::{Bsp, Child, LeafId, LeafKind};
use crate::polytope;
use mscent_math::{DPlane, Polygon, Split, epsilon};

/// An opening between two empty leaves.
#[derive(Clone, Debug, PartialEq)]
pub struct Portal {
    /// The shared surface, wound counter-clockwise seen from `front`.
    pub polygon: Polygon,
    /// Leaf on the side the polygon's normal points to.
    pub front: LeafId,
    /// Leaf on the other side.
    pub back: LeafId,
}

impl Portal {
    /// The leaf across the portal from `from`, or `None` if `from` is not an
    /// end of this portal.
    pub fn other(&self, from: LeafId) -> Option<LeafId> {
        if from == self.front {
            Some(self.back)
        } else if from == self.back {
            Some(self.front)
        } else {
            None
        }
    }
}

/// All portals of a level plus a per-leaf adjacency list.
#[derive(Clone, Debug, PartialEq)]
pub struct Portals {
    portals: Vec<Portal>,
    /// For each leaf, indices into `portals` of the portals it borders.
    adjacency: Vec<Vec<u32>>,
}

impl Portals {
    /// Generates the portals of `bsp`.
    pub fn build(bsp: &Bsp) -> Self {
        let mut found = Vec::new();
        let mut cell = Vec::new();
        walk(bsp, bsp.root(), &mut cell, &mut found);
        let mut adjacency = vec![Vec::new(); bsp.leaves().len()];
        let mut portals = Vec::with_capacity(found.len());
        for p in found {
            // Portal counts are bounded by the u32 node limit times a small
            // factor; saturating keeps the index well-defined regardless.
            let index = u32::try_from(portals.len()).unwrap_or(u32::MAX);
            for leaf in [p.front, p.back] {
                if let Some(list) = adjacency.get_mut(leaf.0 as usize) {
                    list.push(index);
                }
            }
            portals.push(p);
        }
        Self { portals, adjacency }
    }

    pub fn portals(&self) -> &[Portal] {
        &self.portals
    }

    /// Indices (into [`Portals::portals`]) of the portals bordering `leaf`;
    /// empty for unknown or solid leaves.
    pub fn leaf_portals(&self, leaf: LeafId) -> &[u32] {
        self.adjacency
            .get(leaf.0 as usize)
            .map_or(&[], Vec::as_slice)
    }

    /// Leaves directly reachable from `leaf` through one portal.
    pub fn neighbors(&self, leaf: LeafId) -> impl Iterator<Item = LeafId> + '_ {
        self.leaf_portals(leaf)
            .iter()
            .filter_map(move |&i| self.portals.get(i as usize)?.other(leaf))
    }

    /// Every leaf reachable from `start` (including it), in breadth-first
    /// order. Empty if `start` is not a leaf of the level.
    pub fn flood_fill(&self, start: LeafId) -> Vec<LeafId> {
        if start.0 as usize >= self.adjacency.len() {
            return Vec::new();
        }
        let mut seen = vec![false; self.adjacency.len()];
        seen[start.0 as usize] = true;
        let mut order = vec![start];
        let mut head = 0;
        while let Some(&leaf) = order.get(head) {
            head += 1;
            for next in self.neighbors(leaf) {
                if let Some(flag) = seen.get_mut(next.0 as usize)
                    && !*flag
                {
                    *flag = true;
                    order.push(next);
                }
            }
        }
        order
    }
}

/// Visits every node with the planes (oriented outward) bounding its cell.
fn walk(bsp: &Bsp, child: Child, cell: &mut Vec<DPlane>, out: &mut Vec<Portal>) {
    let Child::Node(id) = child else { return };
    let Some(node) = bsp.node(id) else { return };
    if let Some(poly) = node_face(&node.plane, cell) {
        portals_of_node(bsp, node.plane, node.front, node.back, poly, out);
    }
    cell.push(node.plane.flipped());
    walk(bsp, node.front, cell, out);
    cell.pop();
    cell.push(node.plane);
    walk(bsp, node.back, cell, out);
    cell.pop();
}

/// The part of `plane` inside `cell`. The node plane goes first so that a
/// duplicate of it among the ancestors counts as redundant, not as a
/// contradiction.
fn node_face(plane: &DPlane, cell: &[DPlane]) -> Option<Polygon> {
    let mut planes = Vec::with_capacity(cell.len() + 1);
    planes.push(*plane);
    planes.extend_from_slice(cell);
    polytope::faces(&planes)
        .into_iter()
        .find_map(|(i, poly)| (i == 0).then_some(poly))
}

fn portals_of_node(
    bsp: &Bsp,
    plane: DPlane,
    front: Child,
    back: Child,
    poly: Polygon,
    out: &mut Vec<Portal>,
) {
    let mut fronts = Vec::new();
    locate(bsp, front, poly, 1.0, &plane, &mut fronts);
    for (front_leaf, frag) in fronts {
        if bsp.leaf_kind(front_leaf) != LeafKind::Empty {
            continue;
        }
        let mut backs = Vec::new();
        locate(bsp, back, frag, -1.0, &plane, &mut backs);
        for (back_leaf, piece) in backs {
            if bsp.leaf_kind(back_leaf) == LeafKind::Empty && !is_sliver(&piece) {
                out.push(Portal {
                    polygon: piece,
                    front: front_leaf,
                    back: back_leaf,
                });
            }
        }
    }
}

fn is_sliver(p: &Polygon) -> bool {
    p.area() < epsilon::MIN_POLYGON_AREA
}

/// Pushes `poly` down `child`, collecting `(leaf, fragment)` for every leaf
/// it touches. `probe` is the sign of the side of `source` being examined:
/// a polygon lying exactly in a node's plane follows the side that `probe`
/// points to, since that is the side of the surface the caller is looking at.
fn locate(
    bsp: &Bsp,
    child: Child,
    poly: Polygon,
    probe: f64,
    source: &DPlane,
    out: &mut Vec<(LeafId, Polygon)>,
) {
    match child {
        Child::Leaf(id) => out.push((id, poly)),
        Child::Node(id) => {
            let Some(n) = bsp.node(id) else { return };
            match poly.split_by_plane(&n.plane) {
                Split::Front => locate(bsp, n.front, poly, probe, source, out),
                Split::Back => locate(bsp, n.back, poly, probe, source, out),
                Split::On => {
                    let toward = n.plane.normal.dot(source.normal) * probe;
                    let next = if toward > 0.0 { n.front } else { n.back };
                    locate(bsp, next, poly, probe, source, out);
                }
                Split::Spanning { front, back } => {
                    locate(bsp, n.front, front, probe, source, out);
                    locate(bsp, n.back, back, probe, source, out);
                }
            }
        }
    }
}
