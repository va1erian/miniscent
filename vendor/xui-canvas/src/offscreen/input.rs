#![forbid(unsafe_code)]

//! Input into an offscreen window, delivered as the windowed backend delivers
//! it: the pointer leaving a node tells that node (`MouseLeave`), a press moves
//! the keyboard focus to the node it lands on (`KillFocus` to the old holder,
//! `SetFocus` to the new one; a press on bare window takes it from a popup),
//! and `Backend::focus` does the same, so a test sees the focus rings, carets
//! and hover states of a real session.

use xui_core::backend::{Event, WidgetId, WindowId};

use super::OffscreenBackend;
use super::geometry::translate;
use crate::geometry::{absolute_bounds, hit_test};
use crate::scene::event_marks_target;

impl OffscreenBackend {
    /// Sends `event` to `target`'s window sink, then marks `target`'s place
    /// stale when the event is one that can change how it looks, so a widget
    /// that forgets to invalidate cannot leave old pixels behind (see
    /// [`event_marks_target`]).
    pub(super) fn deliver(&self, window: WindowId, target: WidgetId, event: &Event) -> bool {
        let sink = self
            .windows
            .borrow()
            .get(&window.raw())
            .and_then(|entry| entry.sink.clone());
        let handled = sink.is_some_and(|sink| sink.deliver(target, event));
        if !target.is_none() && event_marks_target(event) {
            self.nodes.borrow_mut().invalidate(target);
        }
        handled
    }

    /// Delivers a positionless event to the focused node of `window`, or to no
    /// node when none is focused there, so the app's shortcut hook still sees
    /// a key.
    fn inject_to_focus(&self, window: WindowId, event: &Event) -> bool {
        let target = (*self.focused.borrow())
            .filter(|target| self.nodes.borrow().window_of(*target) == Some(window))
            .unwrap_or(WidgetId::NONE);
        self.deliver(window, target, event)
    }

    /// Moves the keyboard focus to `id`: the old holder gets `KillFocus`, `id`
    /// gets `SetFocus`. This is `Backend::focus`.
    pub(super) fn focus_node(&self, id: WidgetId) {
        let Some(window) = self.nodes.borrow().window_of(id) else {
            return;
        };
        let previous = self.focused.borrow_mut().replace(id);
        self.kill_focus(previous, id);
        self.deliver(window, id, &Event::SetFocus);
    }

    /// Tells `previous`, the node that held the focus, that `id` took it.
    fn kill_focus(&self, previous: Option<WidgetId>, id: WidgetId) {
        let Some(previous) = previous.filter(|previous| *previous != id) else {
            return;
        };
        let window = self.nodes.borrow().window_of(previous);
        if let Some(window) = window {
            self.deliver(window, previous, &Event::KillFocus);
        }
    }

    /// A press on bare window, where no node can take the focus: a popup
    /// surface of `window` that holds it loses it (`KillFocus`), as a press
    /// outside the popup dismisses it. Any other holder keeps the focus.
    fn dismiss_popup_focus(&self, window: WindowId) {
        let popup = {
            let nodes = self.nodes.borrow();
            let mut focused = self.focused.borrow_mut();
            let popup = focused.filter(|id| {
                nodes.window_of(*id) == Some(window) && nodes.get(*id).is_some_and(|n| n.popup)
            });
            if popup.is_some() {
                *focused = None;
            }
            popup
        };
        if let Some(popup) = popup {
            self.deliver(window, popup, &Event::KillFocus);
        }
    }

    /// The press of a pointer button on `id`: a window system focuses the node
    /// it lands on before the node sees the press, and a node that already has
    /// the focus is not told again.
    fn focus_on_press(&self, window: WindowId, id: WidgetId) {
        let previous = self.focused.borrow_mut().replace(id);
        self.kill_focus(previous, id);
        if previous != Some(id) {
            self.deliver(window, id, &Event::SetFocus);
        }
    }

    /// The pointer is now over `target` (or over nothing): the node it was
    /// over before is told it left.
    fn set_hover(&self, window: WindowId, target: Option<WidgetId>) {
        let previous = self
            .windows
            .borrow_mut()
            .get_mut(&window.raw())
            .and_then(|entry| std::mem::replace(&mut entry.hover, target));
        let Some(previous) = previous.filter(|previous| Some(*previous) != target) else {
            return;
        };
        let exists = self.nodes.borrow().contains(previous);
        if exists {
            self.deliver(window, previous, &Event::MouseLeave);
        }
    }

    /// Delivers `event` as a window system would, returning whether it was
    /// consumed.
    ///
    /// A pointer event goes to the topmost node under its position, and a
    /// captured node receives every pointer move and release, even outside its
    /// bounds. A move over a different node first tells the node it left
    /// (`MouseLeave`), and a press first moves the keyboard focus to the node
    /// it lands on (`KillFocus`, `SetFocus`), as the windowed backend does. An
    /// event without a position (a key or a character) goes to the focused
    /// node in `window`, if any.
    pub fn inject(&self, window: WindowId, event: Event) -> bool {
        let Some((x, y)) = event.position() else {
            return self.inject_to_focus(window, &event);
        };
        let captured = *self.captured.borrow();
        let target = match captured {
            Some(id) => {
                let nodes = self.nodes.borrow();
                let abs = absolute_bounds(&nodes, id);
                abs.map(|abs| (id, x - abs.left, y - abs.top))
            }
            None => hit_test(&self.nodes.borrow(), window, x, y),
        };
        if matches!(event, Event::MouseMove { .. }) && captured.is_none() {
            self.set_hover(window, target.map(|(id, _, _)| id));
        }
        let Some((target, lx, ly)) = target else {
            if matches!(
                event,
                Event::MouseDown { .. } | Event::MouseDoubleClick { .. }
            ) && captured.is_none()
            {
                self.dismiss_popup_focus(window);
            }
            return false;
        };
        if matches!(
            event,
            Event::MouseDown { .. } | Event::MouseDoubleClick { .. }
        ) {
            self.focus_on_press(window, target);
        }
        let event = translate(event, lx, ly);
        self.deliver(window, target, &event)
    }
}
