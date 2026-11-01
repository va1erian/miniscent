#![forbid(unsafe_code)]

//! The scene mutations that change what is painted, each telling the window's
//! damage which rectangles it can have changed.
//!
//! A change dirties the node's *old and new* pixels: the part of the window its
//! painting (and its descendants') covered before, and the part it covers
//! after. That is the node's absolute bounds trimmed to its ancestors' clips,
//! for itself and every node inside it, hidden subtrees excluded. The damage
//! list grows each rectangle by the overdraw margin
//! ([`margin`](crate::compositor::margin)) when it is taken.
//!
//! The mutators return whether they changed anything, so a backend asks for a
//! frame only when one is owed.

use xui_core::backend::{Painter, WidgetId, WindowId};
use xui_core::geometry::Rect;

use super::Scene;
use super::damage::Damage;
use crate::geometry::{Placement, intersect, placement, union};

impl Scene {
    /// The damage of `window`, created stale: a window nothing was painted in
    /// is wholly stale.
    fn damage_of(&mut self, window: WindowId) -> &mut Damage {
        self.damage.entry(window.raw()).or_insert_with(Damage::full)
    }

    /// Marks `rect`, in `window` coordinates, stale.
    pub(super) fn note(&mut self, window: WindowId, rect: Rect) {
        self.damage_of(window).add(rect);
    }

    /// Marks the whole of `window` stale: a resize, a DPI or theme change, new
    /// GL content. Always correct, never cheap.
    pub(crate) fn invalidate_window(&mut self, window: WindowId) {
        self.damage_of(window).add_all();
    }

    /// Takes `window`'s damage as rectangles inside `bounds` to repaint, each
    /// grown by `margin` pixels (the overdraw margin at the window's DPI; the
    /// whole of `bounds` when everything is stale or `force` is set), and
    /// resets it. Empty when nothing changed.
    pub(crate) fn take_damage(
        &mut self,
        window: WindowId,
        bounds: Rect,
        force: bool,
        margin: i32,
    ) -> Vec<Rect> {
        self.damage_of(window).take(bounds, force, margin)
    }

    /// The part of the window that `id` and every node inside it paint: the
    /// union of their bounds trimmed to their ancestors' clips, over the
    /// nodes that paint and are visible. Empty for an unknown node.
    pub(super) fn subtree_extent(&self, id: WidgetId) -> Rect {
        let Some(root) = placement(self, id) else {
            return Rect::default();
        };
        let mut extent = Rect::default();
        let mut pending = vec![(id, root)];
        while let Some((member, place)) = pending.pop() {
            if !place.visible {
                // Nothing below a hidden node paints.
                continue;
            }
            let Some(node) = self.get(member) else {
                continue;
            };
            if node.painter.is_some() {
                extent = union(extent, place.visible_bounds());
            }
            for child in self.children(member) {
                if let Some(child_node) = self.get(*child) {
                    pending.push((*child, Placement::of(child_node, Some(&place))));
                }
            }
        }
        extent
    }

    /// The part of the window `id`'s own painting covers.
    pub(super) fn own_extent(&self, id: WidgetId) -> Rect {
        match (self.get(id), placement(self, id)) {
            (Some(node), Some(place)) if node.painter.is_some() => place.visible_bounds(),
            _ => Rect::default(),
        }
    }

    /// Runs `change` on the scene and marks stale both what `id`'s subtree
    /// covered before it and what it covers after.
    fn around_subtree<R>(&mut self, id: WidgetId, change: impl FnOnce(&mut Scene) -> R) -> R {
        let window = self.window_of(id);
        let before = self.subtree_extent(id);
        let result = change(self);
        let after = self.subtree_extent(id);
        if let Some(window) = window {
            self.note(window, before);
            self.note(window, after);
        }
        result
    }

    /// Places `id` at `bounds` (relative to its parent). Returns the previous
    /// bounds, or `None` when there is no such node. The old and new places of
    /// the node and everything inside it are stale.
    pub(crate) fn set_bounds(&mut self, id: WidgetId, bounds: Rect) -> Option<Rect> {
        let previous = self.get(id)?.bounds;
        if previous != bounds {
            self.around_subtree(id, |scene| {
                if let Some(node) = scene.get_mut(id) {
                    node.bounds = bounds;
                }
            });
        }
        Some(previous)
    }

    /// Shows or hides `id` (and with it everything inside). Returns whether
    /// that changed it.
    pub(crate) fn set_visible(&mut self, id: WidgetId, visible: bool) -> bool {
        if self.get(id).is_none_or(|node| node.visible == visible) {
            return false;
        }
        self.around_subtree(id, |scene| {
            if let Some(node) = scene.get_mut(id) {
                node.visible = visible;
            }
        });
        true
    }

    /// Sets the clip on `id`'s descendants. Returns whether that changed it.
    /// What the descendants showed under the old clip and shows under the new
    /// one is stale.
    pub(crate) fn set_clip(&mut self, id: WidgetId, clip: Option<Rect>) -> bool {
        if self.get(id).is_none_or(|node| node.clip == clip) {
            return false;
        }
        self.around_subtree(id, |scene| {
            if let Some(node) = scene.get_mut(id) {
                node.clip = clip;
            }
        });
        true
    }

    /// Replaces `id`'s text. Returns whether that changed it; a node whose
    /// painter shows the text is stale then.
    pub(crate) fn set_text(&mut self, id: WidgetId, text: &str) -> bool {
        let Some(node) = self.get_mut(id) else {
            return false;
        };
        if node.text == text {
            return false;
        }
        node.text = text.to_string();
        self.note_own(id);
        true
    }

    /// Sets `id`'s draw routine. The node's own place is stale: the new
    /// painter has not been seen yet.
    pub(crate) fn set_painter(&mut self, id: WidgetId, painter: Painter) -> bool {
        let Some(node) = self.get_mut(id) else {
            return false;
        };
        node.painter = Some(painter);
        self.note_own(id);
        true
    }

    /// Marks `id`'s painted place stale.
    fn note_own(&mut self, id: WidgetId) {
        let extent = self.own_extent(id);
        if let Some(window) = self.window_of(id) {
            self.note(window, extent);
        }
    }

    /// A node asked to be repainted: its whole visible area, whether or not
    /// it has a painter of its own.
    pub(crate) fn invalidate(&mut self, id: WidgetId) -> bool {
        let Some(window) = self.window_of(id) else {
            return false;
        };
        let Some(place) = placement(self, id) else {
            return false;
        };
        self.note(window, place.visible_bounds());
        true
    }

    /// A node asked for `rect`, in its own coordinates, to be repainted. The
    /// rectangle is honoured as given, even where it leaves the node's bounds
    /// (a painter that draws far outside its node says so this way); it is
    /// still trimmed to what the ancestors' clips show.
    pub(crate) fn invalidate_rect(&mut self, id: WidgetId, rect: Rect) -> bool {
        let Some(window) = self.window_of(id) else {
            return false;
        };
        let Some(place) = placement(self, id) else {
            return false;
        };
        if !place.visible {
            return false;
        }
        let mut area = rect.offset(place.bounds.left, place.bounds.top);
        if let Some(clip) = place.clip {
            area = intersect(area, clip);
        }
        self.note(window, area);
        true
    }
}

/// Whether delivering `event` to a node can change how the node looks without
/// the node saying so: a press, a release, a wheel step, a key, a character, a
/// focus change, a capture change or the pointer leaving. The backends mark the
/// target's place stale after delivering one, whether or not the node reported
/// the event handled, so a widget that changes its paint and forgets to
/// invalidate cannot leave stale pixels behind.
///
/// "Handled" is not a reliable signal: a host reports it only when the widget's
/// mapper returned a message, and a widget that repaints in response to a key
/// and returns none has consumed the event all the same. Stale pixels are a
/// correctness bug and extra damage is a cost, so the rule does not look at it.
///
/// A plain `MouseMove` is left out: it is by far the commonest event, hover
/// changes arrive as `MouseLeave` (and as the widget's own invalidate), and
/// marking every node a pointer crosses would repaint a whole control for a
/// pixel of movement.
///
/// The cost, in a random input session over the 43 demos: mean damage about
/// 49.8k px per frame, against 24.0k with no rule. At 1080p, with a card of
/// about 30 widgets, a repainted card is about 0.4 ms on a light theme and
/// about 2.4 ms on Midnight (release, `damage_bench`).
pub(crate) fn event_marks_target(event: &xui_core::backend::Event) -> bool {
    use xui_core::backend::Event;
    matches!(
        event,
        Event::MouseDown { .. }
            | Event::MouseUp { .. }
            | Event::MouseDoubleClick { .. }
            | Event::MouseWheel { .. }
            | Event::MouseLeave
            | Event::CaptureChanged
            | Event::KeyDown { .. }
            | Event::KeyUp { .. }
            | Event::Char(_)
            | Event::TextChanged
            | Event::SetFocus
            | Event::KillFocus
    )
}

#[cfg(test)]
mod tests;
