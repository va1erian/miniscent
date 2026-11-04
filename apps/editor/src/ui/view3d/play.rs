//! What the Play pane does beyond showing frames: a native window of its own,
//! which Play presents into on the GPU so that its frames skip the UI thread
//! and the software composite, and input sent on the moment it arrives.

use std::sync::Arc;

use mscent_platform::Event as PEvent;

use super::{Shared, View3d};
use crate::studio::worker::{Input, Surface};

impl<M: 'static> View3d<M> {
    /// Asks for a native window of the pane's own (`on`), or gives it up.
    pub fn set_native(&self, on: bool) {
        self.ui.set_native_surface(self.control.id(), on);
    }

    /// The pane's native window, once the backend has made it; `None` on one
    /// that has none, where the pane shows the frames it is given.
    pub fn native(&self) -> Option<Surface> {
        let window = self.ui.native_surface(self.control.id())?;
        Some(Arc::new(window))
    }

    /// Sends input to `input` as it arrives, or (`None`) keeps it for
    /// [`View3d::take_events`] again.
    pub fn set_forward(&self, input: Option<Input>) {
        *self.shared.forward.borrow_mut() = input;
    }

    /// Hands over the input that had to wait, if any.
    pub fn flush_forward(&self) {
        if let Some(input) = self.shared.forward.borrow().as_ref() {
            input.send(&[]);
        }
    }
}

impl Shared {
    /// Hands `e` on, or keeps it for the next tick.
    pub(super) fn push(&self, e: PEvent) {
        match self.forward.borrow().as_ref() {
            Some(input) => input.send(&[e]),
            None => self.events.borrow_mut().push(e),
        }
    }
}
