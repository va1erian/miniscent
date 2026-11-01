//! Pointer lock: hiding and grabbing the cursor so a node gets raw relative
//! motion (mouse-look), and giving the cursor back whenever the lock ends.

use winit::window::CursorGrabMode;
use xui_core::backend::{Event, WidgetId};

use super::Shared;

impl Shared {
    /// The OS window of the node that holds the lock, if any.
    fn lock_window(&self, id: WidgetId) -> Option<std::rc::Rc<winit::window::Window>> {
        let window = self.nodes.borrow().window_of(id)?;
        self.windows.borrow().get(&window.raw())?.window.clone()
    }

    /// Hides and grabs the cursor for `id`. `Locked` is preferred; some
    /// platforms only support `Confined`, which still delivers raw motion.
    pub(crate) fn lock_pointer(&self, id: WidgetId) {
        self.unlock_pointer(false);
        let Some(handle) = self.lock_window(id) else {
            return;
        };
        if handle.set_cursor_grab(CursorGrabMode::Locked).is_err()
            && handle.set_cursor_grab(CursorGrabMode::Confined).is_err()
        {
            return;
        }
        handle.set_cursor_visible(false);
        *self.pointer_lock.borrow_mut() = Some(id);
    }

    /// Gives the cursor back. `notify` sends `PointerLockLost` to the node
    /// that held the lock, for a release it did not ask for.
    pub(crate) fn unlock_pointer(&self, notify: bool) {
        let Some(id) = self.pointer_lock.borrow_mut().take() else {
            return;
        };
        if let Some(handle) = self.lock_window(id) {
            // A failed release leaves nothing more to try; the grab ends with
            // the window anyway.
            let _ = handle.set_cursor_grab(CursorGrabMode::None);
            handle.set_cursor_visible(true);
        }
        // Looked up first so the `nodes` borrow is gone when the handler runs.
        let window = self.nodes.borrow().window_of(id);
        if notify && let Some(window) = window {
            self.deliver(window, id, &Event::PointerLockLost);
        }
    }
}
