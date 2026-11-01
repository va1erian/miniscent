#![forbid(unsafe_code)]

//! Node geometry traversal shared by the windowed compositor and the offscreen
//! backend: a node's window-absolute bounds, the clip its ancestors impose,
//! its effective visibility and the hit test, all over a [`Scene`].
//!
//! It lives at the crate root, not under `backend`, so the offscreen backend
//! (which compiles with the `winit-backend` feature off) can share it without
//! pulling in the windowed modules.

use xui_core::backend::{ParentRef, WidgetId, WindowId};
use xui_core::geometry::{Point, Rect};

use crate::scene::{Node, Scene};

/// The window-absolute bounds of `id`, walking its parent chain.
pub(crate) fn absolute_bounds(scene: &Scene, id: WidgetId) -> Option<Rect> {
    let node = scene.get(id)?;
    let (mut left, mut top) = (0, 0);
    let mut parent = node.parent;
    while let ParentRef::Widget(parent_id) = parent {
        let ancestor = scene.get(parent_id)?;
        left += ancestor.bounds.left;
        top += ancestor.bounds.top;
        parent = ancestor.parent;
    }
    Some(node.bounds.offset(left, top))
}

/// The intersection of two rectangles.
pub(crate) fn intersect(a: Rect, b: Rect) -> Rect {
    Rect::new(
        a.left.max(b.left),
        a.top.max(b.top),
        a.right.min(b.right),
        a.bottom.min(b.bottom),
    )
}

/// The smallest rectangle holding `a` and `b`; an empty one is ignored.
pub(crate) fn union(a: Rect, b: Rect) -> Rect {
    if a.is_empty() {
        return b;
    }
    if b.is_empty() {
        return a;
    }
    Rect::new(
        a.left.min(b.left),
        a.top.min(b.top),
        a.right.max(b.right),
        a.bottom.max(b.bottom),
    )
}

/// `clip`, a clip in a node's own coordinates, in window coordinates, given
/// the node's window-absolute bounds `origin`.
pub(crate) fn lift(clip: Rect, origin: Rect) -> Rect {
    Rect::new(
        origin.left + clip.left,
        origin.top + clip.top,
        origin.left + clip.right,
        origin.top + clip.bottom,
    )
}

/// The clip on `id`'s painting from its ancestors' clips ([`Node::clip`]), in
/// window coordinates, or `None` when no ancestor clips it.
///
/// A node's clip is in that node's own coordinate space, so it is lifted to
/// window coordinates with the node's absolute origin; descendants intersect
/// every ancestor's clip.
pub(crate) fn ancestor_clip(scene: &Scene, id: WidgetId) -> Option<Rect> {
    let node = scene.get(id)?;
    let mut clip: Option<Rect> = None;
    let mut parent = node.parent;
    while let ParentRef::Widget(parent_id) = parent {
        let Some(ancestor) = scene.get(parent_id) else {
            break;
        };
        let Some(origin) = absolute_bounds(scene, parent_id) else {
            break;
        };
        if let Some(local) = ancestor.clip {
            let rect = lift(local, origin);
            clip = Some(match clip {
                Some(current) => intersect(current, rect),
                None => rect,
            });
        }
        parent = ancestor.parent;
    }
    clip
}

/// Whether `id` is visible through its whole ancestry: its own flag and every
/// ancestor's. A node under a hidden ancestor is not composited, matching the
/// Win32 child-window cascade.
pub(crate) fn effectively_visible(scene: &Scene, id: WidgetId) -> bool {
    ancestry_allows(scene, id, |node| node.visible)
}

/// Whether `id` is enabled through its whole ancestry: its own flag and every
/// ancestor's. A disabled container disables its children, as a disabled parent
/// window does on Win32.
pub(crate) fn effectively_enabled(scene: &Scene, id: WidgetId) -> bool {
    ancestry_allows(scene, id, |node| node.enabled)
}

/// Whether `id` and every ancestor satisfy `flag`, or `false` when a node on
/// the chain is missing.
fn ancestry_allows(scene: &Scene, id: WidgetId, flag: impl Fn(&Node) -> bool) -> bool {
    let mut current = Some(id);
    while let Some(node_id) = current {
        let Some(node) = scene.get(node_id) else {
            return false;
        };
        if !flag(node) {
            return false;
        }
        current = match node.parent {
            ParentRef::Window(_) => None,
            ParentRef::Widget(parent) => Some(parent),
        };
    }
    true
}

/// The window-absolute bounds of `id` for hit-testing `(x, y)`, or `None` when
/// `id` is not [effectively visible](effectively_visible) and
/// [enabled](effectively_enabled), the point is outside those bounds, or it
/// falls outside an ancestor's clip. The windowed and offscreen hit tests share
/// this so both agree with the paint path.
pub(crate) fn hit_bounds(scene: &Scene, id: WidgetId, x: i32, y: i32) -> Option<Rect> {
    if !effectively_visible(scene, id) || !effectively_enabled(scene, id) {
        return None;
    }
    let bounds = absolute_bounds(scene, id)?;
    let point = Point::new(x, y);
    if !bounds.contains(point) {
        return None;
    }
    if let Some(clip) = ancestor_clip(scene, id)
        && !clip.contains(point)
    {
        return None;
    }
    Some(bounds)
}

/// The topmost node of `window` under `(x, y)` in window coordinates, with
/// the point translated into that node's own client coordinates.
pub(crate) fn hit_test(
    scene: &Scene,
    window: WindowId,
    x: i32,
    y: i32,
) -> Option<(WidgetId, i32, i32)> {
    scene.iter().rev().find_map(|(id, node)| {
        if node.window != window {
            return None;
        }
        let abs = hit_bounds(scene, id, x, y)?;
        Some((id, x - abs.left, y - abs.top))
    })
}

/// Where a node is: its window-absolute bounds, whether it is visible through
/// its whole ancestry, and the clips that apply to it and to its descendants.
/// Resolved from the parent's placement, so a pass over a scene resolves each
/// node once instead of walking its ancestors.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Placement {
    /// Window-absolute bounds.
    pub(crate) bounds: Rect,
    /// Visible through the whole ancestry.
    pub(crate) visible: bool,
    /// The clip that applies to the node itself: its ancestors'.
    pub(crate) clip: Option<Rect>,
    /// The clip that applies to its descendants: `clip` and its own.
    pub(crate) below: Option<Rect>,
}

impl Placement {
    /// `node`'s placement given its parent's (`None` for a node placed in the
    /// window itself).
    pub(crate) fn of(node: &Node, parent: Option<&Placement>) -> Placement {
        let (bounds, visible, clip) = match parent {
            None => (node.bounds, node.visible, None),
            Some(parent) => (
                node.bounds.offset(parent.bounds.left, parent.bounds.top),
                parent.visible && node.visible,
                parent.below,
            ),
        };
        let own = node.clip.map(|local| lift(local, bounds));
        let below = match (clip, own) {
            (Some(outer), Some(own)) => Some(intersect(outer, own)),
            (outer, own) => outer.or(own),
        };
        Placement {
            bounds,
            visible,
            clip,
            below,
        }
    }

    /// The part of the window the node can paint: its bounds trimmed to the
    /// clip. Empty when the node is hidden or the clip leaves nothing of it.
    ///
    /// A node with no extent along one axis (a 0 x H or W x 0 node) still has
    /// a place a painter can draw along or around (a rule, a ring, a caret), so
    /// the axis with no extent counts as one pixel; the other keeps its size.
    pub(crate) fn visible_bounds(&self) -> Rect {
        if !self.visible {
            return Rect::default();
        }
        let mut bounds = self.bounds;
        if bounds.right <= bounds.left {
            bounds.right = bounds.left.saturating_add(1);
        }
        if bounds.bottom <= bounds.top {
            bounds.bottom = bounds.top.saturating_add(1);
        }
        match self.clip {
            Some(clip) => intersect(bounds, clip),
            None => bounds,
        }
    }
}

/// The placement of `id`, walking its parent chain once from the window.
pub(crate) fn placement(scene: &Scene, id: WidgetId) -> Option<Placement> {
    let mut chain = vec![id];
    let mut parent = scene.get(id)?.parent;
    while let ParentRef::Widget(parent_id) = parent {
        chain.push(parent_id);
        parent = scene.get(parent_id)?.parent;
    }
    let mut above: Option<Placement> = None;
    for member in chain.iter().rev() {
        above = Some(Placement::of(scene.get(*member)?, above.as_ref()));
    }
    above
}

#[cfg(test)]
mod tests;
