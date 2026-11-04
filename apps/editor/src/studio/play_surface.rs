//! Handing the render thread the Play pane's own window, and taking it back.

use super::Studio;
use super::worker::{Input, Surface};

impl Studio {
    /// Has Play present into `window`, the Play pane's own, or (`None`) send
    /// frames for the pane again.
    pub fn set_surface(&mut self, window: Option<Surface>) {
        self.surface = window.is_some();
        if let Some(w) = &mut self.worker {
            // A loss reported for an earlier window is not this one's.
            w.take_surface_lost();
            w.surface(window);
        }
    }

    /// How wide the pane's frame may be: the pane's own width on the GPU, and
    /// in Play's own window, which the GPU shows at the window's size as the
    /// game does; software pays for pixels, so it draws narrower and scales up.
    pub(super) fn max_frame_width(&self, pane: (u32, u32)) -> u32 {
        if super::gpu_view::active() || self.surface {
            pane.0
        } else {
            960
        }
    }

    /// Whether Play was handed a window to present into.
    pub fn has_surface(&self) -> bool {
        self.surface
    }

    /// Whether the render thread gave the window up since the last call, which
    /// leaves Play's frames to the pane.
    pub fn take_surface_lost(&mut self) -> bool {
        let lost = self.worker.as_ref().is_some_and(|w| w.take_surface_lost());
        if lost {
            self.surface = false;
        }
        lost
    }

    /// A way into the render thread's input for Play's pane, while there is
    /// a render thread.
    pub fn input_sender(&self) -> Option<Input> {
        self.worker.as_ref().map(|w| w.input_sender())
    }
}
