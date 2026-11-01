#![forbid(unsafe_code)]

//! Repainting part of a [`Surface`]: painters draw in surface coordinates,
//! but only the pixels of one rectangle change.
//!
//! A canvas clip cannot do this: a rectangular clip trims each *shape* to the
//! clip before it is filled or stroked, so a node straddling the clip edge
//! would get a border drawn along it. A region instead paints on a scratch
//! pixmap *as large as the surface*, in the surface's own coordinates, with
//! the region's pixels copied in first and only those copied back. The canvas
//! therefore runs exactly as it does for a whole-surface paint: every shape
//! keeps its real geometry, a curved or slanted edge is rasterised with the
//! same slopes, and a gradient is evaluated at the same coordinates, so the
//! pixels of the region are bit-for-bit those of a whole-surface paint.
//!
//! What a shape draws outside the region lands in scratch pixels that are never
//! copied back. To keep the cost near the region's area, rectangle fills, text
//! and unscaled images are trimmed to the region as they are drawn (which does
//! not change the pixels inside it); other shapes are drawn whole.
//!
//! The scratch is kept between calls and is never cleared: only the region's
//! rows are read, and they are copied in first. It is touched only where
//! something is drawn, so the memory behind it is committed lazily.

use tiny_skia::{IntSize, Pixmap};
use xui_core::geometry::Rect;

use crate::Surface;
use crate::canvas::SkiaCanvas;
use crate::image_cache::ImageCache;
use crate::paint::intersect;

/// The part of a [`Surface`] being repainted by [`Surface::paint_region`].
pub struct Region<'a> {
    /// The surface's own pixmap for a whole-surface region, else the scratch.
    pixmap: &'a mut Pixmap,
    area: Rect,
    images: &'a mut ImageCache,
    /// Whether `pixmap` is the scratch, so shapes may be trimmed to `area`.
    scratch: bool,
}

impl Region<'_> {
    /// The repainted rectangle, in surface coordinates: the requested region
    /// trimmed to the surface.
    pub fn area(&self) -> Rect {
        self.area
    }

    /// Runs `draw` with a canvas over `bounds` (in surface coordinates) at a
    /// dots-per-inch of `dpi`, like [`Surface::with_canvas_at`]; only the
    /// region's pixels change.
    pub fn with_canvas_at<R>(
        &mut self,
        bounds: Rect,
        dpi: u32,
        draw: impl FnOnce(&mut SkiaCanvas) -> R,
    ) -> R {
        let mut canvas = self.canvas(bounds, dpi);
        draw(&mut canvas)
    }

    /// Like [`Region::with_canvas_at`], for a widget painted over its
    /// ancestors, like [`Surface::with_canvas_over_parents`].
    pub fn with_canvas_over_parents<R>(
        &mut self,
        bounds: Rect,
        dpi: u32,
        draw: impl FnOnce(&mut SkiaCanvas) -> R,
    ) -> R {
        let mut canvas = self.canvas(bounds, dpi);
        canvas.over_parents = true;
        draw(&mut canvas)
    }

    fn canvas(&mut self, bounds: Rect, dpi: u32) -> SkiaCanvas<'_> {
        let mut canvas = SkiaCanvas::new(self.pixmap, self.images, bounds, dpi);
        canvas.scissor = self.scratch.then_some(self.area);
        canvas
    }
}

/// Copies the rows `area` covers from one surface-sized pixel buffer to
/// another, `width` pixels a row.
fn copy_area(from: &[u8], to: &mut [u8], width: usize, area: Rect) {
    if area.is_empty() {
        return;
    }
    let run = area.width() as usize * 4;
    for y in area.top as usize..area.bottom as usize {
        let at = (y * width + area.left as usize) * 4;
        to[at..at + run].copy_from_slice(&from[at..at + run]);
    }
}

impl Surface {
    /// Repaints `region` (surface coordinates) of the surface: `paint` draws
    /// through the [`Region`]'s canvases as it would over the whole surface,
    /// but pixels outside `region` are neither changed nor, where it is
    /// cheap to avoid, rasterised.
    ///
    /// The region starts with the surface's current pixels, so painters blend
    /// over them exactly as they would in place, and what it ends with is
    /// bit-for-bit what a whole-surface paint leaves there. A region off the
    /// surface still runs `paint` and changes nothing.
    pub fn paint_region<R>(&mut self, region: Rect, paint: impl FnOnce(&mut Region<'_>) -> R) -> R {
        let (width, height) = self.size();
        let whole = Rect::new(0, 0, width as i32, height as i32);
        let area = intersect(region, whole);
        // A whole-surface repaint (a first frame, a resize) needs no copy.
        if area == whole {
            return paint(&mut Region {
                pixmap: &mut self.pixmap,
                area,
                images: &mut self.images,
                scratch: false,
            });
        }
        let area = if area.is_empty() {
            Rect::new(0, 0, 0, 0)
        } else {
            area
        };
        // The scratch is kept between calls, so a repaint per frame neither
        // allocates nor faults in a fresh pixmap; its rows outside the region
        // are never read, so it is not cleared.
        let wanted = width as usize * height as usize * 4;
        let mut data = std::mem::take(&mut self.scratch);
        if data.len() != wanted {
            data = vec![0; wanted];
        }
        copy_area(self.pixmap.data(), &mut data, width as usize, area);
        let size = IntSize::from_wh(width, height).expect("a surface has a size");
        let mut scratch = Pixmap::from_vec(data, size).expect("sized to the surface");
        let result = paint(&mut Region {
            pixmap: &mut scratch,
            area,
            images: &mut self.images,
            scratch: true,
        });
        let data = scratch.take();
        copy_area(&data, self.pixmap.data_mut(), width as usize, area);
        self.scratch = data;
        result
    }
}
