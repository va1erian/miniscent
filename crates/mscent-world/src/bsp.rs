//! The solid/empty-leaf BSP tree.
//!
//! Internal nodes hold a splitting plane; the *front* child is the half-space
//! where `normal . p - dist > 0`, the *back* child is everything else (so a
//! point exactly on a plane goes back). Leaves are solid or empty. Indices are
//! `u32` newtypes so the tree can be serialised compactly.

use mscent_math::{Aabb, BoxSide, DPlane, Plane, Vec3, Vec3d};

/// Deepest tree any reader or builder accepts; bounds every traversal.
pub const MAX_DEPTH: usize = 256;

/// Index of a node in [`Bsp::nodes`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NodeId(pub u32);

/// Index of a leaf in [`Bsp::leaves`]. Doubles as the leaf's identity.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct LeafId(pub u32);

/// A child link: another node or a leaf.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Child {
    Node(NodeId),
    Leaf(LeafId),
}

/// Whether a leaf is solid rock or empty space.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LeafKind {
    Solid,
    Empty,
}

/// A convex cell of the partition.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Leaf {
    pub kind: LeafKind,
}

/// A splitting plane with its two children.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Node {
    pub plane: DPlane,
    pub front: Child,
    pub back: Child,
}

impl Node {
    /// The plane narrowed to runtime precision.
    pub fn plane_f32(&self) -> Plane {
        self.plane.to_f32()
    }
}

/// Result of classifying a volume against the tree.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Region {
    /// Only solid leaves are touched.
    Solid,
    /// Only empty leaves are touched.
    Empty,
    /// Both kinds are touched.
    Mixed,
}

impl Region {
    fn merge(self, o: Self) -> Self {
        if self == o { self } else { Self::Mixed }
    }
}

/// Why a hand-built tree was rejected.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BspError {
    /// A child, or the root, points past the end of its array.
    IndexOutOfRange,
    /// A node or leaf is reachable more than once (a cycle or shared child).
    Shared,
    /// A node or leaf is not reachable from the root.
    Unreachable,
    /// The tree is deeper than [`MAX_DEPTH`].
    TooDeep,
    /// A plane has a non-finite component.
    NotFinite,
    /// More nodes or leaves than a `u32` index can address.
    TooLarge,
}

/// A validated BSP tree. Construct with [`Bsp::from_parts`] or `csg::compile`.
#[derive(Clone, Debug, PartialEq)]
pub struct Bsp {
    nodes: Vec<Node>,
    leaves: Vec<Leaf>,
    root: Child,
}

impl Bsp {
    /// Validates and wraps raw parts: indices in range, planes finite, a true
    /// tree (each node and leaf reached exactly once), depth within
    /// [`MAX_DEPTH`]. This is the entry point for untrusted data.
    pub fn from_parts(nodes: Vec<Node>, leaves: Vec<Leaf>, root: Child) -> Result<Self, BspError> {
        if u32::try_from(nodes.len()).is_err() || u32::try_from(leaves.len()).is_err() {
            return Err(BspError::TooLarge);
        }
        if nodes
            .iter()
            .any(|n| !n.plane.normal.is_finite() || !n.plane.dist.is_finite())
        {
            return Err(BspError::NotFinite);
        }
        let bsp = Self {
            nodes,
            leaves,
            root,
        };
        bsp.check_structure()?;
        Ok(bsp)
    }

    /// Iterative walk with an explicit stack so hostile depth cannot
    /// overflow the call stack.
    fn check_structure(&self) -> Result<(), BspError> {
        let mut seen_nodes = vec![false; self.nodes.len()];
        let mut seen_leaves = vec![false; self.leaves.len()];
        let mut stack = vec![(self.root, 1usize)];
        while let Some((child, depth)) = stack.pop() {
            if depth > MAX_DEPTH + 1 {
                return Err(BspError::TooDeep);
            }
            match child {
                Child::Leaf(LeafId(i)) => mark(&mut seen_leaves, i)?,
                Child::Node(NodeId(i)) => {
                    mark(&mut seen_nodes, i)?;
                    let n = &self.nodes[i as usize];
                    stack.push((n.back, depth + 1));
                    stack.push((n.front, depth + 1));
                }
            }
        }
        if seen_nodes.iter().chain(&seen_leaves).all(|&s| s) {
            Ok(())
        } else {
            Err(BspError::Unreachable)
        }
    }

    pub fn nodes(&self) -> &[Node] {
        &self.nodes
    }

    pub fn leaves(&self) -> &[Leaf] {
        &self.leaves
    }

    pub fn root(&self) -> Child {
        self.root
    }

    pub fn node(&self, id: NodeId) -> Option<&Node> {
        self.nodes.get(id.0 as usize)
    }

    pub fn leaf(&self, id: LeafId) -> Option<&Leaf> {
        self.leaves.get(id.0 as usize)
    }

    /// Kind of a leaf; an unknown id reads as solid so queries never panic.
    pub fn leaf_kind(&self, id: LeafId) -> LeafKind {
        self.leaf(id).map_or(LeafKind::Solid, |l| l.kind)
    }

    fn leaf_data(&self, id: LeafId) -> Leaf {
        self.leaf(id).copied().unwrap_or(Leaf {
            kind: LeafKind::Solid,
        })
    }

    /// Longest root-to-leaf path counted in nodes.
    pub fn depth(&self) -> usize {
        let mut best = 0;
        let mut stack = vec![(self.root, 0usize)];
        while let Some((child, d)) = stack.pop() {
            match child {
                Child::Leaf(_) => best = best.max(d),
                Child::Node(id) => {
                    if let Some(n) = self.node(id) {
                        stack.push((n.front, d + 1));
                        stack.push((n.back, d + 1));
                    }
                }
            }
        }
        best
    }

    /// The leaf containing `p` (`f64`, build precision).
    pub fn leaf_at(&self, p: Vec3d) -> LeafId {
        self.descend(|plane| plane.signed_distance(p) > 0.0)
    }

    /// The leaf containing `p` using the runtime `f32` planes.
    pub fn leaf_at_f32(&self, p: Vec3) -> LeafId {
        self.descend(|plane| plane.to_f32().signed_distance(p) > 0.0)
    }

    fn descend(&self, goes_front: impl Fn(&DPlane) -> bool) -> LeafId {
        let mut child = self.root;
        // Validation bounds the depth, so the loop always ends; the counter
        // guards trees built by other means anyway.
        for _ in 0..=MAX_DEPTH + 1 {
            match child {
                Child::Leaf(id) => return id,
                Child::Node(id) => {
                    let Some(n) = self.node(id) else { break };
                    child = if goes_front(&n.plane) {
                        n.front
                    } else {
                        n.back
                    };
                }
            }
        }
        LeafId(u32::MAX)
    }

    /// True if `p` lies in a solid leaf.
    pub fn is_solid(&self, p: Vec3d) -> bool {
        self.leaf_kind(self.leaf_at(p)) == LeafKind::Solid
    }

    /// Solid/empty/mixed verdict for every leaf the box overlaps.
    pub fn classify_aabb(&self, b: &Aabb) -> Region {
        self.classify_volume(|plane| match b.classify_plane(&plane.to_f32()) {
            BoxSide::Front => Straddle::Front,
            BoxSide::Back => Straddle::Back,
            BoxSide::Straddling => Straddle::Both,
        })
    }

    /// Same for a sphere of `radius` around `center`.
    pub fn classify_sphere(&self, center: Vec3, radius: f32) -> Region {
        let r = radius.abs();
        self.classify_volume(|plane| {
            let d = plane.to_f32().signed_distance(center);
            if d > r {
                Straddle::Front
            } else if d < -r {
                Straddle::Back
            } else {
                Straddle::Both
            }
        })
    }

    fn classify_volume(&self, side_of: impl Fn(&DPlane) -> Straddle) -> Region {
        let mut result: Option<Region> = None;
        let mut stack = vec![self.root];
        while let Some(child) = stack.pop() {
            let region = match child {
                Child::Leaf(id) => match self.leaf_data(id).kind {
                    LeafKind::Solid => Region::Solid,
                    LeafKind::Empty => Region::Empty,
                },
                Child::Node(id) => {
                    let Some(n) = self.node(id) else { continue };
                    match side_of(&n.plane) {
                        Straddle::Front => stack.push(n.front),
                        Straddle::Back => stack.push(n.back),
                        Straddle::Both => {
                            stack.push(n.front);
                            stack.push(n.back);
                        }
                    }
                    continue;
                }
            };
            result = Some(result.map_or(region, |r| r.merge(region)));
            if result == Some(Region::Mixed) {
                break;
            }
        }
        result.unwrap_or(Region::Solid)
    }
}

enum Straddle {
    Front,
    Back,
    Both,
}

fn mark(seen: &mut [bool], i: u32) -> Result<(), BspError> {
    let slot = seen.get_mut(i as usize).ok_or(BspError::IndexOutOfRange)?;
    if *slot {
        return Err(BspError::Shared);
    }
    *slot = true;
    Ok(())
}
