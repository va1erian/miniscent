#![forbid(unsafe_code)]

//! The state machine behind a window's GL content: [`RendererState`] creates the
//! [`GlSurface`] on the first frame and falls back to software for good when the
//! context cannot be created or a frame cannot be rendered.

use winit::window::Window;
use xui_core::color::Color;
use xui_core::image::Image;

use super::GlSurface;

/// Which renderer currently draws a window's GL content.
pub(crate) enum RendererState {
    /// Not yet decided: the first frame creates the surface or falls back.
    Untried,
    /// Drawing with OpenGL.
    Gl(Box<GlSurface>),
    /// No GL context; the widget's software paint runs instead.
    Software,
}

impl RendererState {
    /// Renders one OpenGL frame of `width` x `height` pixels into an image,
    /// creating the surface on first use. `draw` receives the current
    /// [`glow::Context`] with the viewport covering the offscreen texture and
    /// the texture cleared to `background`.
    ///
    /// The returned image is composited through the software painter model, so
    /// GL content and CPU nodes share one frame. When no context or framebuffer
    /// can be created, `teardown` runs with the context still current (when one
    /// exists) before the renderer falls back to software, so the widget can
    /// free its GPU resources.
    ///
    /// Returns `None` when no GL image was rendered, so the caller paints the
    /// software fallback instead.
    pub(crate) fn render(
        &mut self,
        window: &Window,
        width: u32,
        height: u32,
        background: Color,
        draw: impl FnOnce(&glow::Context),
        teardown: impl FnOnce(&glow::Context),
    ) -> Option<Image> {
        if matches!(self, RendererState::Untried) {
            *self = match GlSurface::new(window) {
                Ok(surface) => RendererState::Gl(Box::new(surface)),
                Err(_) => RendererState::Software,
            };
        }
        let RendererState::Gl(surface) = self else {
            return None;
        };
        match surface.render_offscreen(width, height, background, draw) {
            Some(pixels) => Image::from_rgba(width.max(1), height.max(1), pixels).ok(),
            None => {
                surface.with_gl(teardown);
                *self = RendererState::Software;
                None
            }
        }
    }

    /// Runs `f` with the current [`glow::Context`] if a surface exists, without
    /// dropping it. Use it to free one widget's GPU objects while other GL
    /// content shares the same surface. Returns whether a context was available.
    pub(crate) fn with_gl(&self, f: impl FnOnce(&glow::Context)) -> bool {
        if let RendererState::Gl(surface) = self {
            surface.with_gl(f);
            true
        } else {
            false
        }
    }

    /// Drops the OpenGL surface, if one exists, first running `teardown` with
    /// its context current. Use it when the GL content is removed so GPU
    /// resources are freed while the context is still live.
    pub(crate) fn teardown_gl(&mut self, teardown: impl FnOnce(&glow::Context)) {
        if let RendererState::Gl(surface) = self {
            surface.with_gl(teardown);
            *self = RendererState::Untried;
        }
    }
}
