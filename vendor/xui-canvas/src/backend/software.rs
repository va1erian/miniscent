#![forbid(unsafe_code)]

//! The software presentation of a real window: a `softbuffer` context and
//! surface, created lazily on the first software frame, and the window's
//! persistent frame.

use std::num::NonZeroU32;
use std::rc::Rc;

use winit::window::Window;

use super::SharedWindow;
use crate::Surface;

/// A real window's presentation state.
///
/// Created lazily on the first presented frame. Even a window with GL content
/// owns one now: the GL widget is rendered into an offscreen framebuffer and
/// composited through this surface, and its context is never swapped, so
/// `softbuffer` remains the sole presenter and the two do not compete for the
/// `HWND`.
pub(crate) struct RealWindow {
    /// The `softbuffer` context and surface.
    pub(crate) software: Software,
    /// The window's frame: every pixel of the last frame painted, repainted
    /// from damage and kept between frames. `None` before the first frame.
    pub(crate) frame: Option<Surface>,
    /// Whether the `softbuffer` buffer may lack the last painted frame (a
    /// frame was painted but not handed over), so the next present must send
    /// the whole frame rather than only the damage.
    pub(crate) buffer_stale: bool,
}

/// A window's `softbuffer` context and surface.
pub(crate) struct Software {
    context: Option<softbuffer::Context<SharedWindow>>,
    surface: Option<softbuffer::Surface<SharedWindow, SharedWindow>>,
    /// The size `surface` was last resized to.
    sized: Option<(u32, u32)>,
}

impl RealWindow {
    /// A window with no software surface yet.
    pub(crate) fn new() -> RealWindow {
        RealWindow {
            software: Software {
                context: None,
                surface: None,
                sized: None,
            },
            frame: None,
            buffer_stale: false,
        }
    }
}

impl Software {
    /// Creates the software surface on first use. Returns whether a surface is
    /// available.
    pub(crate) fn ensure(&mut self, window: &Rc<Window>) -> bool {
        if self.surface.is_some() {
            return true;
        }
        let Ok(context) = softbuffer::Context::new(SharedWindow(Rc::clone(window))) else {
            return false;
        };
        let Ok(surface) = softbuffer::Surface::new(&context, SharedWindow(Rc::clone(window)))
        else {
            return false;
        };
        self.context = Some(context);
        self.surface = Some(surface);
        true
    }

    /// The software surface sized to `width` x `height` (resized only when
    /// that changed), once [`Software::ensure`] has created it. `None` when it
    /// cannot be created or sized.
    pub(crate) fn sized(
        &mut self,
        width: NonZeroU32,
        height: NonZeroU32,
    ) -> Option<&mut softbuffer::Surface<SharedWindow, SharedWindow>> {
        let surface = self.surface.as_mut()?;
        if self.sized != Some((width.get(), height.get())) {
            surface.resize(width, height).ok()?;
            self.sized = Some((width.get(), height.get()));
        }
        Some(surface)
    }
}
