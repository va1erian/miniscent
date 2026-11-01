#![forbid(unsafe_code)]

//! The retained scene both canvas backends keep: the nodes the front layer
//! created, by id, with their paint order and their parent/child links.
//!
//! A window's nodes paint in creation order (later on top) and
//! [`Backend::raise`](xui_core::backend::Backend::raise) moves a node to the
//! end of that order. The scene keeps the order as a sequence number per node
//! in an ordered map, and the nodes in a hash map, so a lookup by id, a raise,
//! a destroy and a walk down a subtree are logarithmic or linear in what they
//! touch instead of a scan of every node, and a frame can plan its nodes in
//! one pass (see [`crate::compositor`]).
//!
//! It lives at the crate root, not under `backend`, so the offscreen backend
//! (which compiles with the `winit-backend` feature off) shares it.

use std::collections::{BTreeMap, HashMap};

mod damage;
mod notes;

use damage::Damage;
pub(crate) use notes::event_marks_target;

use xui_core::backend::{Painter, ParentRef, WidgetId, WindowId};
use xui_core::geometry::Rect;

/// One node the front layer created.
pub(crate) struct Node {
    /// The window the node belongs to.
    pub(crate) window: WindowId,
    /// What the node is placed relative to: the window or a container node.
    pub(crate) parent: ParentRef,
    /// The bounds, relative to the parent.
    pub(crate) bounds: Rect,
    /// Whether the node itself is shown (ignoring its ancestors).
    pub(crate) visible: bool,
    /// Whether the node itself is enabled (ignoring its ancestors).
    pub(crate) enabled: bool,
    /// The text the front layer stored on the node.
    pub(crate) text: String,
    /// The draw routine, once the widget registered one.
    pub(crate) painter: Option<Painter>,
    /// Whether a press that starts here drags the whole window (the windowed
    /// backend's custom caption).
    #[cfg(feature = "winit-backend")]
    pub(crate) drag_region: bool,
    /// A clip on this node's descendants, in the node's own coordinate space.
    pub(crate) clip: Option<Rect>,
    /// Whether the node is a transient popup surface, which a press outside it
    /// dismisses by taking the keyboard focus from it.
    pub(crate) popup: bool,
}

impl Node {
    /// A visible, enabled node with no painter, text, clip or drag behaviour.
    pub(crate) fn new(window: WindowId, parent: ParentRef, bounds: Rect) -> Node {
        Node {
            window,
            parent,
            bounds,
            visible: true,
            enabled: true,
            text: String::new(),
            painter: None,
            #[cfg(feature = "winit-backend")]
            drag_region: false,
            clip: None,
            popup: false,
        }
    }
}

struct Slot {
    node: Node,
    /// The node's place in the paint order.
    seq: u64,
}

/// The nodes of every window of one backend.
#[derive(Default)]
pub(crate) struct Scene {
    nodes: HashMap<WidgetId, Slot>,
    /// Paint order: ascending sequence number.
    order: BTreeMap<u64, WidgetId>,
    next_seq: u64,
    /// The nodes created directly inside a container node, in no order.
    children: HashMap<WidgetId, Vec<WidgetId>>,
    /// What changed in each window (by raw id) since its last painted frame.
    damage: HashMap<u64, Damage>,
}

impl Scene {
    /// An empty scene.
    pub(crate) fn new() -> Scene {
        Scene::default()
    }

    /// How many nodes the scene holds, across windows.
    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.nodes.len()
    }

    /// Adds `node` as `id`, on top of every node there is.
    pub(crate) fn insert(&mut self, id: WidgetId, node: Node) {
        if self.nodes.contains_key(&id) {
            self.unlink(id);
        }
        if let ParentRef::Widget(parent) = node.parent {
            self.children.entry(parent).or_default().push(id);
        }
        let seq = self.next_seq;
        self.next_seq += 1;
        self.order.insert(seq, id);
        self.nodes.insert(id, Slot { node, seq });
    }

    /// The node `id`, if there is one.
    pub(crate) fn get(&self, id: WidgetId) -> Option<&Node> {
        self.nodes.get(&id).map(|slot| &slot.node)
    }

    /// The node `id`, mutably. Its `parent` must not be changed.
    pub(crate) fn get_mut(&mut self, id: WidgetId) -> Option<&mut Node> {
        self.nodes.get_mut(&id).map(|slot| &mut slot.node)
    }

    /// Whether the node `id` exists.
    pub(crate) fn contains(&self, id: WidgetId) -> bool {
        self.nodes.contains_key(&id)
    }

    /// The window owning the node `id`.
    pub(crate) fn window_of(&self, id: WidgetId) -> Option<WindowId> {
        self.get(id).map(|node| node.window)
    }

    /// Every node in paint order, bottom first; `.rev()` is top first.
    pub(crate) fn iter(&self) -> impl DoubleEndedIterator<Item = (WidgetId, &Node)> {
        self.order.values().filter_map(|id| {
            let slot = self.nodes.get(id)?;
            Some((*id, &slot.node))
        })
    }

    /// Every node, mutably, in no particular order.
    #[cfg(feature = "winit-backend")]
    pub(crate) fn nodes_mut(&mut self) -> impl Iterator<Item = &mut Node> {
        self.nodes.values_mut().map(|slot| &mut slot.node)
    }

    /// The nodes created directly inside `id`, in no particular order.
    pub(crate) fn children(&self, id: WidgetId) -> &[WidgetId] {
        self.children.get(&id).map_or(&[], Vec::as_slice)
    }

    /// `id` and every node created inside it, parents before children.
    pub(crate) fn subtree(&self, id: WidgetId) -> Vec<WidgetId> {
        let mut members = Vec::new();
        if !self.contains(id) {
            return members;
        }
        let mut pending = vec![id];
        while let Some(next) = pending.pop() {
            members.push(next);
            pending.extend_from_slice(self.children(next));
        }
        members
    }

    /// Moves `id` above every other node. Returns whether it moved (it did
    /// not if it was already on top or is not there).
    pub(crate) fn raise(&mut self, id: WidgetId) -> bool {
        let Some(slot) = self.nodes.get_mut(&id) else {
            return false;
        };
        if self.order.last_key_value().map(|(_, top)| *top) == Some(id) {
            return false;
        }
        self.order.remove(&slot.seq);
        slot.seq = self.next_seq;
        self.next_seq += 1;
        self.order.insert(slot.seq, id);
        // Its pixels now paint above whatever it overlaps.
        if let Some(window) = self.window_of(id) {
            let extent = self.subtree_extent(id);
            self.note(window, extent);
        }
        true
    }

    /// Removes `id` and every node created inside it, and returns their ids
    /// (empty when `id` is not there).
    pub(crate) fn remove(&mut self, id: WidgetId) -> Vec<WidgetId> {
        // What the subtree covered is stale once it is gone.
        if let Some(window) = self.window_of(id) {
            let extent = self.subtree_extent(id);
            self.note(window, extent);
        }
        let removed = self.subtree(id);
        if let Some(first) = removed.first() {
            self.unlink_from_parent(*first);
        }
        for gone in &removed {
            if let Some(slot) = self.nodes.remove(gone) {
                self.order.remove(&slot.seq);
            }
            self.children.remove(gone);
        }
        removed
    }

    /// Removes every node of `window`, and returns their ids.
    pub(crate) fn remove_window(&mut self, window: WindowId) -> Vec<WidgetId> {
        let gone: Vec<WidgetId> = self
            .nodes
            .iter()
            .filter(|(_, slot)| slot.node.window == window)
            .map(|(id, _)| *id)
            .collect();
        for id in &gone {
            if let Some(slot) = self.nodes.remove(id) {
                self.order.remove(&slot.seq);
            }
            self.children.remove(id);
        }
        self.damage.remove(&window.raw());
        // A surviving container (another window's) cannot hold one of these,
        // since a node's parent is in its own window.
        gone
    }

    /// Drops `id` from its parent's child list.
    fn unlink_from_parent(&mut self, id: WidgetId) {
        let Some(ParentRef::Widget(parent)) = self.get(id).map(|node| node.parent) else {
            return;
        };
        if let Some(siblings) = self.children.get_mut(&parent) {
            siblings.retain(|sibling| *sibling != id);
            if siblings.is_empty() {
                self.children.remove(&parent);
            }
        }
    }

    /// Forgets a node that is about to be replaced by an insert of its id.
    fn unlink(&mut self, id: WidgetId) {
        self.unlink_from_parent(id);
        if let Some(slot) = self.nodes.remove(&id) {
            self.order.remove(&slot.seq);
        }
    }
}

#[cfg(test)]
mod tests;
