#![forbid(unsafe_code)]

//! The application-facing seam: a [`GlWidget`] draws itself with either OpenGL
//! or the software [`Canvas`], so a window stays usable when no GL context can
//! be created.

use xui_core::backend::Canvas;
use xui_core::{Rect, Theme};

/// A GPU-rendered widget.
///
/// Install one with
/// [`WinitBackend::set_gl_content`](crate::WinitBackend::set_gl_content) to
/// cover the whole client area, or
/// [`WinitBackend::set_gl_content_on`](crate::WinitBackend::set_gl_content_on)
/// to fill one node's pane and be laid out like any other widget. Each frame the
/// backend renders [`GlWidget::paint_gl`] into a texture, reads it back and
/// composites it through the same software surface as every CPU node, so GL
/// content is one painter among many and the window's ordinary widgets draw
/// with it. When no GL context can be created (a headless session, no driver),
/// or a frame cannot be rendered, the backend paints [`GlWidget::paint`] into
/// the software surface instead, so the window keeps working.
///
/// The widget is shared (`&self`), so state that changes while painting lives in
/// `Cell`/`RefCell` fields.
pub trait GlWidget: 'static {
    /// Paints the software fallback into `canvas`, covering `bounds` (in device
    /// pixels, in the window's coordinates: the whole client area for
    /// window-level content, the node's rectangle for node-level content). Draw
    /// only from `theme`'s semantic tokens.
    ///
    /// Called only when no GL context is available; a widget should mirror its
    /// GPU output closely enough to stay readable. The default draws nothing.
    fn paint(&self, canvas: &mut dyn Canvas, bounds: Rect, theme: &Theme);

    /// Paints one OpenGL frame. `gl` is the window's context, made current with
    /// the viewport set to `bounds`' size and an offscreen framebuffer cleared
    /// to the theme background; issue GL calls through it and the backend reads
    /// the frame back and composites it at `bounds` when this returns. Do not
    /// present the frame yourself; do not assume the default framebuffer is
    /// bound, and leave the backend's framebuffer bound if you bind your own
    /// (the readback happens straight after this returns).
    fn paint_gl(&self, gl: &glow::Context, bounds: Rect, theme: &Theme);

    /// Releases the GPU resources created in [`GlWidget::paint_gl`].
    ///
    /// Called with `gl` made current, just before the window's
    /// [`GlSurface`](crate::GlSurface) is dropped: when the window closes, or
    /// when a failed frame makes the backend fall back to software. Free
    /// anything that must be destroyed with the context current here. The
    /// default does nothing.
    fn gl_teardown(&self, gl: &glow::Context) {
        let _ = gl;
    }
}
