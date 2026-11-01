//! CSG: merging ordered add/subtract brushes into a [`Bsp`].
//!
//! The world starts as one solid leaf. Each brush, in ascending `order`, is
//! pushed down the tree. Wherever the brush overlaps a leaf of the other kind
//! that leaf is split by exactly those brush planes that cut it, and the
//! inside piece takes the brush's kind (`Add` -> solid, `Subtract` -> empty).
//! Overlap is decided by building the convex polytope "cell of the leaf
//! intersect brush" (see [`crate::polytope`]), which is slow but exact enough
//! to be easy to trust: correctness over speed.

use crate::brush::{Brush, Op};
use crate::bsp::{Bsp, BspError, Child, Leaf, LeafId, LeafKind, MAX_DEPTH, Node, NodeId};
use crate::polytope;
use mscent_math::{DPlane, Vec3d};

/// Most brushes one build accepts.
pub const MAX_BRUSHES: usize = 65_536;

/// Most nodes one build may create before giving up.
pub const MAX_NODES: usize = 1 << 22;

/// Why compilation failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CsgError {
    TooManyBrushes,
    /// The tree would exceed [`MAX_DEPTH`].
    TooDeep,
    /// The tree would exceed [`MAX_NODES`].
    TooManyNodes,
    /// The built tree failed validation (an internal bug if ever seen).
    Invalid(BspError),
}

/// Compiles brushes into a BSP. Brushes are applied in ascending `order`,
/// ties keep slice order. The world starts solid.
pub fn compile(brushes: &[Brush]) -> Result<Bsp, CsgError> {
    if brushes.len() > MAX_BRUSHES {
        return Err(CsgError::TooManyBrushes);
    }
    let mut sorted: Vec<&Brush> = brushes.iter().collect();
    sorted.sort_by_key(|b| b.order());
    let mut builder = Builder {
        nodes: Vec::new(),
        leaves: vec![Leaf {
            kind: LeafKind::Solid,
        }],
    };
    let mut root = Child::Leaf(LeafId(0));
    for brush in sorted {
        root = builder.insert(root, &mut Vec::new(), brush)?;
    }
    Bsp::from_parts(builder.nodes, builder.leaves, root).map_err(CsgError::Invalid)
}

struct Builder {
    nodes: Vec<Node>,
    leaves: Vec<Leaf>,
}

/// True if the cell `cell` intersect the brush intersect `extra` has volume.
fn overlaps(cell: &[DPlane], brush: &Brush, extra: Option<DPlane>) -> bool {
    let mut planes = Vec::with_capacity(cell.len() + brush.planes().len() + 1);
    planes.extend_from_slice(cell);
    planes.extend_from_slice(brush.planes());
    planes.extend(extra);
    polytope::has_volume(&planes)
}

/// A box around every legal brush. The root cell is unbounded, and an
/// unbounded region has no faces to count, so cuts are tested inside this box.
pub(crate) fn world_box() -> [DPlane; 6] {
    let r = 2.0 * polytope::MAX_COORD;
    let axis = |x, y, z| DPlane::new(Vec3d::new(x, y, z), r);
    [
        axis(1.0, 0.0, 0.0),
        axis(-1.0, 0.0, 0.0),
        axis(0.0, 1.0, 0.0),
        axis(0.0, -1.0, 0.0),
        axis(0.0, 0.0, 1.0),
        axis(0.0, 0.0, -1.0),
    ]
}

fn target_kind(op: Op) -> LeafKind {
    match op {
        Op::Add => LeafKind::Solid,
        Op::Subtract => LeafKind::Empty,
    }
}

impl Builder {
    /// Applies `brush` inside the cell `cell` (planes whose back sides
    /// bound it, oriented outward) rooted at `child`; returns the new child.
    fn insert(
        &mut self,
        child: Child,
        cell: &mut Vec<DPlane>,
        brush: &Brush,
    ) -> Result<Child, CsgError> {
        match child {
            Child::Node(id) => {
                self.insert_node(id, cell, brush)?;
                Ok(child)
            }
            Child::Leaf(id) => self.insert_leaf(id, cell, brush),
        }
    }

    fn insert_node(
        &mut self,
        id: NodeId,
        cell: &mut Vec<DPlane>,
        brush: &Brush,
    ) -> Result<(), CsgError> {
        let node = self.nodes[id.0 as usize];
        // The front half-space, as an outward constraint, is the flipped plane.
        let front_bound = node.plane.flipped();
        if overlaps(cell, brush, Some(front_bound)) {
            cell.push(front_bound);
            let new = self.insert(node.front, cell, brush);
            cell.pop();
            self.nodes[id.0 as usize].front = new?;
        }
        if overlaps(cell, brush, Some(node.plane)) {
            cell.push(node.plane);
            let new = self.insert(node.back, cell, brush);
            cell.pop();
            self.nodes[id.0 as usize].back = new?;
        }
        Ok(())
    }

    fn insert_leaf(
        &mut self,
        id: LeafId,
        cell: &[DPlane],
        brush: &Brush,
    ) -> Result<Child, CsgError> {
        let old = self.leaves[id.0 as usize].kind;
        let target = target_kind(brush.op());
        if old == target {
            return Ok(Child::Leaf(id));
        }
        // Keep only the brush planes that actually cut what is left of the
        // cell, so unrelated planes never bloat the tree.
        let mut inside: Vec<DPlane> = cell.to_vec();
        inside.extend(world_box());
        let mut cuts = Vec::new();
        for &plane in brush.planes() {
            let front = plane.flipped();
            if polytope::has_volume(&[inside.as_slice(), &[front]].concat()) {
                cuts.push(plane);
                inside.push(plane);
            }
        }
        if cuts.is_empty() {
            // The brush covers the whole cell.
            self.leaves[id.0 as usize].kind = target;
            return Ok(Child::Leaf(id));
        }
        if cell.len() + cuts.len() > MAX_DEPTH {
            return Err(CsgError::TooDeep);
        }
        let mut current = self.new_leaf(target);
        // Build the chain from the innermost cut outwards. The first (outermost)
        // front piece reuses the old leaf id so no leaf is orphaned.
        for (i, plane) in cuts.iter().enumerate().rev() {
            let front = if i == 0 {
                Child::Leaf(id)
            } else {
                self.new_leaf(old)
            };
            current = self.new_node(Node {
                plane: *plane,
                front,
                back: current,
            })?;
        }
        Ok(current)
    }

    fn new_leaf(&mut self, kind: LeafKind) -> Child {
        self.leaves.push(Leaf { kind });
        Child::Leaf(LeafId((self.leaves.len() - 1) as u32))
    }

    fn new_node(&mut self, node: Node) -> Result<Child, CsgError> {
        if self.nodes.len() >= MAX_NODES {
            return Err(CsgError::TooManyNodes);
        }
        self.nodes.push(node);
        Ok(Child::Node(NodeId((self.nodes.len() - 1) as u32)))
    }
}
