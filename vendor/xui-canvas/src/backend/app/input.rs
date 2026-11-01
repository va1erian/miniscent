#![forbid(unsafe_code)]

//! Pointer, cursor and keyboard translation: tracking the cursor, hit-testing,
//! focus, pointer capture and modifier state, and turning `winit` events into
//! the portable [`Event`] vocabulary.

use std::rc::Rc;
use std::time::Instant;

use winit::event::{ElementState, MouseButton as WinitButton, MouseScrollDelta};
use winit::keyboard::Key as WinitKey;
use winit::window::{CursorIcon, ResizeDirection, Window};

use xui_core::backend::{Decorations, Event, WidgetId, WindowId};
use xui_core::message::MouseButton;

use super::App;
use super::keymap::{
    RESIZE_BORDER_PX, cursor_icon, key_events, mouse_button, resize_cursor, resize_direction,
};

impl App<'_> {
    /// The window's live `winit` window, if it has one yet.
    fn window_handle(&self, raw: u64) -> Option<Rc<Window>> {
        self.shared
            .windows
            .borrow()
            .get(&raw)
            .and_then(|state| state.window.clone())
    }

    /// The native resize direction for a pointer at `(x, y)`, but only for a
    /// borderless window: a decorated one has system borders that already do
    /// this, and its client coordinates do not include them.
    fn resize_direction_at(&self, raw: u64, x: i32, y: i32) -> Option<ResizeDirection> {
        let (width, height, resizable, borderless) = {
            let windows = self.shared.windows.borrow();
            let state = windows.get(&raw)?;
            (
                state.size.0,
                state.size.1,
                state.resizable,
                state.decorations == Decorations::None,
            )
        };
        if !borderless {
            return None;
        }
        resize_direction(x, y, width, height, RESIZE_BORDER_PX, resizable)
    }

    /// Shows `icon` on the window the pointer is over.
    fn set_window_cursor(&self, raw: u64, icon: CursorIcon) {
        if let Some(handle) = self.window_handle(raw) {
            handle.set_cursor(icon);
        }
    }

    /// Replaces the hovered node, sending `MouseLeave` to the one left behind.
    fn set_hover(&self, raw: u64, window: WindowId, target: Option<WidgetId>) {
        let previous = self
            .shared
            .windows
            .borrow_mut()
            .get_mut(&raw)
            .and_then(|state| std::mem::replace(&mut state.hover, target));
        if previous != target
            && let Some(previous) = previous
        {
            self.shared.deliver(window, previous, &Event::MouseLeave);
        }
    }

    /// Moves the keyboard focus to `id`, telling the old and new holders.
    fn set_focus(&self, raw: u64, id: WidgetId) {
        let previous = self
            .shared
            .windows
            .borrow_mut()
            .get_mut(&raw)
            .and_then(|state| state.focused.replace(id));
        if let Some(previous) = previous
            && previous != id
        {
            self.shared
                .deliver(Self::window_id(raw), previous, &Event::KillFocus);
        }
        if previous != Some(id) {
            self.shared
                .deliver(Self::window_id(raw), id, &Event::SetFocus);
        }
    }

    /// Takes the keyboard focus from a popup surface that holds it, who is told
    /// `KillFocus`: a press where no node can take the focus (bare window, a
    /// drag region, the resize frame) is a press outside the popup. Any other
    /// holder keeps the focus, so typing carries on after such a press.
    fn dismiss_popup_focus(&self, raw: u64) {
        let popup = {
            let mut windows = self.shared.windows.borrow_mut();
            windows
                .get_mut(&raw)
                .filter(|state| state.focused.is_some_and(|id| self.shared.is_popup(id)))
                .and_then(|state| state.focused.take())
        };
        if let Some(popup) = popup {
            self.shared
                .deliver(Self::window_id(raw), popup, &Event::KillFocus);
        }
    }

    /// Shows the cursor requested for the node under the pointer.
    pub(super) fn apply_cursor(&self, raw: u64, target: Option<WidgetId>) {
        let icon = match target {
            Some(id) => self
                .shared
                .cursors
                .borrow()
                .get(&id.raw())
                .copied()
                .map(cursor_icon)
                .unwrap_or(CursorIcon::Default),
            None => CursorIcon::Default,
        };
        self.set_window_cursor(raw, icon);
    }

    pub(super) fn cursor_moved(&mut self, raw: u64, x: f64, y: f64) {
        self.cursor = (x, y);
        // A locked pointer reports relative motion instead of positions.
        if self.shared.pointer_lock.borrow().is_some() {
            return;
        }
        let window = Self::window_id(raw);
        let (x, y) = (x as i32, y as i32);
        // A captured node keeps receiving moves (and the cursor) even outside
        // its bounds, so a drag survives leaving it.
        // Copied out first: the handler may take or release capture.
        let captured = *self.shared.captured.borrow();
        if let Some(captured) = captured {
            if let Some((lx, ly)) = self.shared.local_point(captured, x, y) {
                self.shared.deliver(
                    window,
                    captured,
                    &Event::MouseMove {
                        x: lx,
                        y: ly,
                        modifiers: self.modifiers,
                    },
                );
            }
            self.apply_cursor(raw, Some(captured));
            return;
        }
        // The frame of a borderless resizable window resizes it, so the pointer
        // shows a resize cursor and does not reach the node under it.
        if let Some(direction) = self.resize_direction_at(raw, x, y) {
            self.set_hover(raw, window, None);
            self.set_window_cursor(raw, resize_cursor(direction));
            return;
        }
        let target = self.shared.hit_test(window, x, y);
        let target_id = target.map(|(id, _, _)| id);
        self.set_hover(raw, window, target_id);
        if let Some((id, lx, ly)) = target {
            self.shared.deliver(
                window,
                id,
                &Event::MouseMove {
                    x: lx,
                    y: ly,
                    modifiers: self.modifiers,
                },
            );
        }
        self.apply_cursor(raw, target_id);
    }

    pub(super) fn mouse_input(&mut self, raw: u64, state: ElementState, button: WinitButton) {
        let window = Self::window_id(raw);
        let (x, y) = (self.cursor.0 as i32, self.cursor.1 as i32);
        // A locked pointer's hidden cursor may drift, so buttons go to the
        // node that holds the lock, as under a capture.
        let captured = *self.shared.captured.borrow();
        let captured = captured.or(*self.shared.pointer_lock.borrow());
        let Some(button) = mouse_button(button) else {
            return;
        };
        // A left-button press in the frame of a borderless resizable window
        // starts a native resize, as the system border would on a decorated one.
        if state == ElementState::Pressed
            && button == MouseButton::Left
            && captured.is_none()
            && let Some(direction) = self.resize_direction_at(raw, x, y)
            && let Some(handle) = self.window_handle(raw)
        {
            self.dismiss_popup_focus(raw);
            let _ = handle.drag_resize_window(direction);
            return;
        }
        let target = match captured {
            Some(id) => self
                .shared
                .local_point(id, x, y)
                .map(|(lx, ly)| (id, lx, ly)),
            None => self.shared.hit_test(window, x, y),
        };
        let Some((id, lx, ly)) = target else {
            if state == ElementState::Pressed && captured.is_none() {
                self.dismiss_popup_focus(raw);
            }
            return;
        };
        // A left-button press on a drag region moves the whole window, as a
        // title bar's empty area does, instead of reaching the node.
        if state == ElementState::Pressed
            && button == MouseButton::Left
            && captured.is_none()
            && self.shared.is_drag_region(id)
            && let Some(handle) = self.window_handle(raw)
        {
            self.dismiss_popup_focus(raw);
            let _ = handle.drag_window();
            return;
        }
        let event = if state == ElementState::Pressed {
            self.set_focus(raw, id);
            // `winit` never reports a double-click, so recognize the second
            // press of one here: deliver `MouseDoubleClick` in place of the
            // second `MouseDown`, as Win32's `WM_*BUTTONDBLCLK` does.
            let kind = self.double_click.press(Instant::now(), x, y, button, id);
            kind.event(lx, ly, button, self.modifiers)
        } else {
            Event::MouseUp {
                x: lx,
                y: ly,
                button,
                modifiers: self.modifiers,
            }
        };
        self.shared.deliver(window, id, &event);
    }

    pub(super) fn mouse_wheel(&mut self, raw: u64, delta: MouseScrollDelta) {
        let window = Self::window_id(raw);
        let (x, y) = (self.cursor.0 as i32, self.cursor.1 as i32);
        let locked = *self.shared.pointer_lock.borrow();
        let hit = match locked {
            Some(id) => self
                .shared
                .local_point(id, x, y)
                .map(|(lx, ly)| (id, lx, ly)),
            None => self.shared.hit_test(window, x, y),
        };
        let Some((id, lx, ly)) = hit else {
            return;
        };
        // A line is one wheel notch; `winit` reports fractions, so scale.
        let (dx, dy) = match delta {
            MouseScrollDelta::LineDelta(x, y) => ((x * 120.0) as i32, (y * 120.0) as i32),
            MouseScrollDelta::PixelDelta(position) => (position.x as i32, position.y as i32),
        };
        if dy == 0 && dx == 0 {
            return;
        }
        let horizontal = dy == 0;
        self.shared.deliver(
            window,
            id,
            &Event::MouseWheel {
                delta: (if horizontal { dx } else { dy }).clamp(-32768, 32767) as i16,
                horizontal,
                x: lx,
                y: ly,
                modifiers: self.modifiers,
            },
        );
    }

    pub(super) fn keyboard_input(
        &mut self,
        raw: u64,
        state: ElementState,
        repeat: bool,
        logical: &WinitKey,
        text: Option<&str>,
    ) {
        let window = Self::window_id(raw);
        let focused = self
            .shared
            .windows
            .borrow()
            .get(&raw)
            .and_then(|state| state.focused);
        // With no focused node the keys still reach the app, which offers them
        // to its shortcut hook (a menu accelerator works wherever focus sits).
        let id = focused.unwrap_or(WidgetId::NONE);
        let pressed = state == ElementState::Pressed;
        for event in key_events(pressed, repeat, logical, text, self.modifiers) {
            self.shared.deliver(window, id, &event);
        }
    }
}
