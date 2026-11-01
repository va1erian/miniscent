#![warn(missing_docs)]

//! The cross-platform, software-rendered backend for xui.
//!
//! It rasterises the portable widgets with `tiny-skia` into an RGBA buffer, so
//! the same widget code that runs on the Win32 backend draws here without any
//! platform UI toolkit. An [`OffscreenBackend`] renders the same widgets
//! headlessly for tests and snapshots; the software painter core ([`Surface`],
//! [`SkiaCanvas`] and the `cosmic-text` shaper) is all an app needs to drive its
//! own presentation.
//!
//! With the default `winit-backend` feature a [`WinitBackend`] presents the
//! surface in a real `winit` window through `softbuffer`, and a window can host
//! a GPU renderer: install a [`GlWidget`] with
//! [`WinitBackend::set_gl_content`] and the backend renders `glow` OpenGL
//! frames through a [`GlSurface`] into a texture, reads them back and
//! composites them through the same software path as every CPU node, so GL
//! content is one painter among many. Every module but `sys::gl` forbids
//! `unsafe`; the GL context and loader's `unsafe` is isolated there. Turn the
//! feature off (`default-features = false`) on a target with no windowing
//! system: the crate then builds over `xui-core`, `tiny-skia` and `cosmic-text`
//! alone and exposes the software painter core. Fonts can be registered from
//! memory with [`set_default_font`]/[`add_font`] instead of scanning the system
//! font directories.

mod canvas;
mod clock;
mod compositor;
mod geometry;
mod image_cache;
mod offscreen;
mod paint;
mod region;
mod scene;
pub mod snapshot;
mod text;
mod text_layout;

#[cfg(feature = "winit-backend")]
mod backend;
#[cfg(feature = "winit-backend")]
mod clipboard;
#[cfg(feature = "winit-backend")]
mod gl;
#[cfg(feature = "winit-backend")]
mod sys;

#[cfg(feature = "winit-backend")]
pub use backend::WinitBackend;
pub use canvas::SkiaCanvas;
pub use clock::MonotonicClock;
#[cfg(feature = "winit-backend")]
pub use gl::{GlError, GlSurface, GlWidget};
pub use offscreen::OffscreenBackend;
pub use offscreen::PresentedFrame;
pub use region::Region;
pub use text::{add_font, measure as measure_text, set_default_family, set_default_font};

/// The OpenGL binding a [`GlWidget`] draws with.
#[cfg(feature = "winit-backend")]
pub use glow;

use tiny_skia::Pixmap;
use xui_core::color::Color;
use xui_core::geometry::Rect;

/// An RGBA image rendered by the canvas backend.
#[derive(Clone, Debug)]
pub struct RgbaImage {
    /// Image width in pixels.
    pub width: u32,
    /// Image height in pixels.
    pub height: u32,
    /// Top-down RGBA pixels (row-major, 4 bytes each).
    pub pixels: Vec<u8>,
}

impl RgbaImage {
    /// The pixel at `(x, y)`, if in bounds.
    pub fn pixel(&self, x: u32, y: u32) -> Option<[u8; 4]> {
        if x >= self.width || y >= self.height {
            return None;
        }
        let at = ((y * self.width + x) * 4) as usize;
        self.pixels
            .get(at..at + 4)
            .map(|p| [p[0], p[1], p[2], p[3]])
    }
}

/// A software surface: a rectangle being painted, filled from a background and
/// then drawn into with a [`SkiaCanvas`]. The surface also carries the decoded
/// images its painters draw, so an image uploads once and every repaint
/// reuses it.
pub struct Surface {
    pixmap: Pixmap,
    images: image_cache::ImageCache,
    /// The scratch pixmap of [`Surface::paint_region`], kept for the next.
    scratch: Vec<u8>,
}

impl Surface {
    /// A transparent surface of `width` x `height` pixels.
    pub fn new(width: u32, height: u32) -> Surface {
        let mut pixmap = Pixmap::new(width.max(1), height.max(1)).expect("pixmap");
        pixmap.fill(tiny_skia::Color::TRANSPARENT);
        Surface {
            pixmap,
            images: image_cache::ImageCache::new(),
            scratch: Vec::new(),
        }
    }

    /// The surface's size in pixels, `(width, height)`.
    pub fn size(&self) -> (u32, u32) {
        (self.pixmap.width(), self.pixmap.height())
    }

    /// Fills the whole surface with `color`.
    pub fn fill(&mut self, color: Color) {
        self.pixmap.fill(to_skia(color));
    }

    /// Runs `draw` with a canvas over `bounds` (in surface coordinates),
    /// clipped to that rectangle, at a dots-per-inch of `dpi`.
    pub fn with_canvas_at<R>(
        &mut self,
        bounds: Rect,
        dpi: u32,
        draw: impl FnOnce(&mut SkiaCanvas) -> R,
    ) -> R {
        let mut canvas = SkiaCanvas::new(&mut self.pixmap, &mut self.images, bounds, dpi);
        draw(&mut canvas)
    }

    /// Like [`Surface::with_canvas_at`], for a widget painted after its
    /// ancestors in the same frame: the canvas reports
    /// [`Canvas::composites_parents`](xui_core::Canvas::composites_parents),
    /// so the widget draws on its container instead of filling its own
    /// background.
    pub fn with_canvas_over_parents<R>(
        &mut self,
        bounds: Rect,
        dpi: u32,
        draw: impl FnOnce(&mut SkiaCanvas) -> R,
    ) -> R {
        let mut canvas = SkiaCanvas::new(&mut self.pixmap, &mut self.images, bounds, dpi);
        canvas.over_parents = true;
        draw(&mut canvas)
    }

    /// Runs `draw` with a canvas at a dots-per-inch of 96.
    pub fn with_canvas<R>(&mut self, bounds: Rect, draw: impl FnOnce(&mut SkiaCanvas) -> R) -> R {
        self.with_canvas_at(bounds, 96, draw)
    }

    /// Reads the surface back as an [`RgbaImage`].
    pub fn to_image(&self) -> RgbaImage {
        RgbaImage {
            width: self.pixmap.width(),
            height: self.pixmap.height(),
            pixels: self.pixmap.data().to_vec(),
        }
    }

    /// The surface's top-down RGBA pixels (row-major, 4 bytes each), borrowed
    /// so a caller that already owns a destination can present a frame without
    /// [`to_image`](Surface::to_image)'s whole-pixmap clone.
    ///
    /// The slice is `width * height * 4` bytes, where the dimensions are the
    /// ones the surface was created with (a surface is never resized).
    pub fn pixels(&self) -> &[u8] {
        self.pixmap.data()
    }
}

/// Converts a core [`Color`] to a tiny-skia colour.
pub(crate) fn to_skia(color: Color) -> tiny_skia::Color {
    tiny_skia::Color::from_rgba8(color.r, color.g, color.b, 255)
}

/// Converts a portable [`Rgba`](xui_core::backend::Rgba) to a tiny-skia colour,
/// alpha included.
pub(crate) fn to_skia_rgba(color: xui_core::backend::Rgba) -> tiny_skia::Color {
    tiny_skia::Color::from_rgba8(color.r, color.g, color.b, color.a)
}

#[cfg(test)]
mod clip_tests;
#[cfg(test)]
mod path_tests;
#[cfg(test)]
mod region_tests;
#[cfg(test)]
mod styled_tests;
#[cfg(test)]
mod tests;
