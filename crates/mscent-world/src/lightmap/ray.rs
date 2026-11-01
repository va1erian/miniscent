//! Segment queries against the BSP, used for shadow rays.

use crate::bsp::{Bsp, Child, LeafKind};
use mscent_math::Vec3d;

/// Deep enough for any validated tree: one pending far half per level plus
/// the segment being processed.
const STACK_HINT: usize = crate::bsp::MAX_DEPTH + 2;

/// True if the segment `a`..`b` passes only through empty leaves.
///
/// The segment is split at every node plane it crosses, so no sampling step
/// can skip a thin wall. A start or end point inside solid is blocked, and a
/// dangling node index counts as blocked (the safe answer for shadows).
pub fn segment_clear(bsp: &Bsp, a: Vec3d, b: Vec3d) -> bool {
    if !a.is_finite() || !b.is_finite() {
        return false;
    }
    let mut stack: Vec<(Child, Vec3d, Vec3d)> = Vec::with_capacity(STACK_HINT);
    stack.push((bsp.root(), a, b));
    while let Some((child, p0, p1)) = stack.pop() {
        let id = match child {
            Child::Leaf(id) => {
                if bsp.leaf_kind(id) == LeafKind::Solid {
                    return false;
                }
                continue;
            }
            Child::Node(id) => id,
        };
        let Some(node) = bsp.node(id) else {
            return false;
        };
        let d0 = node.plane.signed_distance(p0);
        let d1 = node.plane.signed_distance(p1);
        // Front is strictly positive; zero belongs to the back, as in leaf_at.
        match (d0 > 0.0, d1 > 0.0) {
            (true, true) => stack.push((node.front, p0, p1)),
            (false, false) => stack.push((node.back, p0, p1)),
            (start_front, _) => {
                let t = d0 / (d0 - d1);
                let mid = p0.lerp(p1, t);
                let (near, far) = if start_front {
                    (node.front, node.back)
                } else {
                    (node.back, node.front)
                };
                stack.push((far, mid, p1));
                stack.push((near, p0, mid));
            }
        }
    }
    true
}
