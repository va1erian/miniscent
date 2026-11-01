#![forbid(unsafe_code)]

//! Frames of an offscreen window: [`OffscreenBackend::present`] repaints what
//! changed on the window's persistent surface and lends its pixels, and
//! [`OffscreenBackend::render`] copies them into an image.

use std::cell::RefCell;
use std::collections::HashSet;

use xui_core::Rect;
use xui_core::backend::WindowId;

use super::OffscreenBackend;
use crate::compositor::{self, Content as _, FrameInfo};
use crate::{RgbaImage, Surface};

/// A frame of an offscreen window, lent by [`OffscreenBackend::present`].
#[derive(Clone, Copy, Debug)]
pub struct PresentedFrame<'a> {
    /// The window's width in pixels.
    pub width: u32,
    /// The window's height in pixels.
    pub height: u32,
    /// The whole frame: top-down RGBA, row-major, 4 bytes a pixel
    /// (`width * height * 4` bytes), over an opaque background.
    pub pixels: &'a [u8],
    /// The window rectangles repainted to produce it, in window pixels. Empty
    /// when nothing changed since the previous frame; the whole window for the
    /// first frame, after a theme change or while GL content is installed.
    pub damage: &'a [Rect],
}

/// Clears a window's in-progress render flag when a frame finishes, however the
/// render returns.
struct RenderGuard<'a> {
    rendering: &'a RefCell<HashSet<u64>>,
    raw: u64,
}

impl Drop for RenderGuard<'_> {
    fn drop(&mut self) {
        self.rendering.borrow_mut().remove(&self.raw);
    }
}

/// Puts a window's surface back after compositing, on every exit path
/// (including a panic in a painter), so a later frame finds it.
struct SurfaceRestore<'a> {
    backend: &'a OffscreenBackend,
    raw: u64,
    surface: Option<Surface>,
}

impl Drop for SurfaceRestore<'_> {
    fn drop(&mut self) {
        if let Some(surface) = self.surface.take()
            && let Some(entry) = self.backend.windows.borrow_mut().get_mut(&self.raw)
        {
            entry.surface = Some(surface);
        }
    }
}

impl OffscreenBackend {
    /// Repaints what changed in `window` since the last frame, on the
    /// window's persistent surface, and lends the whole frame to `f`: no
    /// allocation and no copy of the frame, and nothing painted at all when
    /// nothing changed.
    ///
    /// Only the damaged rectangles are repainted ([`PresentedFrame::damage`]);
    /// the pixels are exactly those of a repaint of the whole window. A
    /// change the backend is told about (a node moved, shown, hidden, raised,
    /// destroyed, a text or painter set, `invalidate`, `invalidate_rect`, a
    /// theme change) marks its old and new places; a window with GL content
    /// is repainted whole every frame. Returns `None` for a window that is not
    /// there, or when called again for the same window from inside `f` or a
    /// painter.
    pub fn present<R>(
        &self,
        window: WindowId,
        f: impl FnOnce(PresentedFrame<'_>) -> R,
    ) -> Option<R> {
        // A painter may ask to capture its own window, which would call
        // `present` again; refuse the nested frame rather than recursing
        // forever. The guard clears this on every exit, including the early
        // `?` below.
        if !self.rendering.borrow_mut().insert(window.raw()) {
            return None;
        }
        let _guard = RenderGuard {
            rendering: &self.rendering,
            raw: window.raw(),
        };
        // Take the surface out of the window state so a painter that re-enters
        // the backend (for example `Ui::dpi`, which reads this same map) does
        // not find it borrowed. The window stays in the map, so painters still
        // read its live DPI, theme and GL content. The GL content is
        // snapshotted in the same step, so a widget's fallback paint can
        // re-enter the backend without the window map staying borrowed.
        let (dpi, theme, surface, gl) = {
            let mut windows = self.windows.borrow_mut();
            let entry = windows.get_mut(&window.raw())?;
            (
                entry.dpi,
                entry.theme,
                entry.surface.take()?,
                entry.gl.snapshot(),
            )
        };
        // The guard puts the surface back on every exit, including a panic in
        // a painter.
        let mut restore = SurfaceRestore {
            backend: self,
            raw: window.raw(),
            surface: Some(surface),
        };
        let surface = restore.surface.as_mut().expect("a taken surface");
        let (width, height) = surface.size();
        let window_rect = Rect::new(0, 0, width as i32, height as i32);
        // GL content is one painter among many, exactly as in the windowed
        // backend; the offscreen backend has no GPU, so it paints the widget's
        // software fallback: window-level content as the base layer, node-level
        // content at its node's bounds. It can change behind the compositor's
        // back, so a window with any is repainted whole.
        let areas = self.nodes.borrow_mut().take_damage(
            window,
            window_rect,
            gl.is_live(),
            compositor::margin(dpi),
        );
        if !areas.is_empty() {
            let plan = compositor::plan(&self.nodes.borrow(), window, dpi, &gl);
            let frame = FrameInfo {
                window: window_rect,
                dpi,
                theme: &theme,
            };
            plan.paint(surface, &areas, &frame, &gl);
        }
        let result = f(PresentedFrame {
            width,
            height,
            pixels: surface.pixels(),
            damage: &areas,
        });
        drop(restore);
        Some(result)
    }

    /// Renders `window`'s visible nodes, in creation order, into an image.
    ///
    /// Only what changed since the previous frame is repainted (see
    /// [`OffscreenBackend::present`]); the image is a copy of the whole frame.
    pub fn render(&self, window: WindowId) -> Option<RgbaImage> {
        self.present(window, |frame| RgbaImage {
            width: frame.width,
            height: frame.height,
            pixels: frame.pixels.to_vec(),
        })
    }

    /// Marks the whole of `window` stale, so the next frame repaints all of
    /// it. The backend marks what a node or theme change touches by itself;
    /// this is for a test that changed what a painter draws behind its back
    /// (a value a painter reads that no widget invalidated).
    pub fn invalidate_window(&self, window: WindowId) {
        self.nodes.borrow_mut().invalidate_window(window);
    }
}
