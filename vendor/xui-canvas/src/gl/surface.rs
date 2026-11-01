#![forbid(unsafe_code)]

//! [`GlSurface`]: an OpenGL context bound to a `winit` window.
//!
//! `glutin`'s `unsafe` lives in [`crate::sys::gl`]; this is the safe view a
//! [`GlWidget`](crate::GlWidget) draws through. The frame lifecycle — make the
//! context current, apply the viewport, clear, and the offscreen readback the
//! compositor uses — is owned by [`xui_gpu::GlSurface`]; this only binds it to
//! the `glutin` context. The backend never swaps this context: every GL frame is
//! composited through the software surface and presented by `softbuffer`.

use winit::window::Window;

use xui_core::color::Color;

use crate::sys::gl::Context;

pub use xui_gpu::GlError;

/// An OpenGL context for one window.
///
/// Created lazily by the backend's GL content path and resized from the window's
/// size in physical pixels; the context is made current on the UI thread when a
/// frame begins.
pub struct GlSurface(xui_gpu::GlSurface);

impl GlSurface {
    /// Creates a context on `window`'s client area. Fails when no GL display,
    /// config or context can be created (a headless session, no driver), so the
    /// caller can fall back to software.
    pub fn new(window: &Window) -> Result<GlSurface, GlError> {
        let size = window.inner_size();
        let pixels = (size.width.max(1), size.height.max(1));
        let context = Context::new(window, pixels.0, pixels.1).map_err(GlError::from)?;
        Ok(GlSurface(xui_gpu::GlSurface::new(
            Box::new(context),
            pixels,
            96,
        )))
    }

    /// Resizes the framebuffer to `width` x `height` physical pixels; the
    /// viewport is applied when the next frame begins.
    pub fn resize(&self, width: i32, height: i32) {
        self.0.resize(width, height);
    }

    /// Records the window's dots-per-inch, for a widget that reads it.
    /// OpenGL draws in physical pixels, so this does not scale the viewport.
    pub fn set_dpi(&self, dpi: u32) {
        self.0.set_dpi(dpi);
    }

    /// The window's dots-per-inch.
    pub fn dpi(&self) -> u32 {
        self.0.dpi()
    }

    /// Starts a frame: makes the context current, resizes the surface and the
    /// viewport to the framebuffer, clears to `background` and returns the
    /// `glow` context to draw with.
    pub fn begin_frame(&self, background: Color) -> &glow::Context {
        self.0.begin_frame(background)
    }

    /// Makes the context current and runs `f` with it, without starting a
    /// frame (no resize, viewport or clear). Use it for GL work outside a
    /// paint, such as freeing GPU resources.
    pub fn with_gl<R>(&self, f: impl FnOnce(&glow::Context) -> R) -> R {
        self.0.with_gl(f)
    }

    /// Presents the frame.
    pub fn end_frame(&self) -> Result<(), GlError> {
        self.0.end_frame()
    }

    /// Renders a widget into a texture and reads it back as top-down RGBA
    /// pixels, so the backend can composite the GL content through its ordinary
    /// software painter model. `None` when no framebuffer could be created, so
    /// the caller uses the widget's software fallback.
    pub(crate) fn render_offscreen(
        &self,
        width: u32,
        height: u32,
        background: Color,
        draw: impl FnOnce(&glow::Context),
    ) -> Option<Vec<u8>> {
        self.0.render_offscreen(width, height, background, draw)
    }
}
