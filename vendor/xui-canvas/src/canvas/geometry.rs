#![forbid(unsafe_code)]

//! [`SkiaCanvas`]'s transform, clip-mask and primitive-drawing helpers, split
//! from `canvas.rs` so both files stay under the size limit.

use tiny_skia::{Mask, Path, PathBuilder, Shader, Transform};

use xui_core::backend::{Corner, PathPlacement, PathSeg, Stroke};
use xui_core::geometry::{Point, Rect};

use super::SkiaCanvas;
use crate::paint::{
    Clip, FILL_RULE, intersect, paint, rect_path, sk_rect, skia_stroke, solid_shader,
};

impl SkiaCanvas<'_> {
    pub(super) fn point(&self, point: Point) -> Point {
        Point::new(
            (self.tx + point.x as f32 * self.scale).round() as i32,
            (self.ty + point.y as f32 * self.scale).round() as i32,
        )
    }

    /// Maps a point in the canvas's own coordinates to device space, keeping
    /// sub-pixel precision.
    pub(super) fn device(&self, (x, y): (f32, f32)) -> (f32, f32) {
        (self.tx + x * self.scale, self.ty + y * self.scale)
    }

    /// Builds `path` placed by `at` in device space, keeping sub-pixel
    /// coordinates so curves stay smooth.
    pub(super) fn placed_path(&self, path: &[PathSeg], at: PathPlacement) -> Option<Path> {
        let map = |x: f32, y: f32| {
            let (x, y) = at.apply(x, y);
            (self.tx + x * self.scale, self.ty + y * self.scale)
        };
        let mut builder = PathBuilder::new();
        for seg in path {
            match *seg {
                PathSeg::MoveTo(x, y) => {
                    let (x, y) = map(x, y);
                    builder.move_to(x, y);
                }
                PathSeg::LineTo(x, y) => {
                    let (x, y) = map(x, y);
                    builder.line_to(x, y);
                }
                PathSeg::CubicTo(x1, y1, x2, y2, x, y) => {
                    let (c1, c2, end) = (map(x1, y1), map(x2, y2), map(x, y));
                    builder.cubic_to(c1.0, c1.1, c2.0, c2.1, end.0, end.1);
                }
                PathSeg::Close => builder.close(),
            }
        }
        builder.finish()
    }

    /// Maps `rect` into device space, ignoring the clip. Text alignment is
    /// relative to the whole target rect, so it must not be trimmed first.
    pub(super) fn map_rect(&self, rect: Rect) -> Rect {
        Rect::new(
            (self.tx + rect.left as f32 * self.scale).round() as i32,
            (self.ty + rect.top as f32 * self.scale).round() as i32,
            (self.tx + rect.right as f32 * self.scale).round() as i32,
            (self.ty + rect.bottom as f32 * self.scale).round() as i32,
        )
    }

    /// The device-space rectangle every pixel is limited to: the innermost
    /// clip's bounds, trimmed to the reach the compositor gave the painter.
    /// `None` when neither is set.
    pub(super) fn limit(&self) -> Option<Rect> {
        let clip = self.clips.last().map(|clip| clip.bounds());
        match (clip, self.reach) {
            (Some(clip), Some(reach)) => Some(intersect(clip, reach)),
            (clip, reach) => clip.or(reach),
        }
    }

    /// Maps `rect` into device space and trims it to the limit.
    pub(super) fn rect(&self, rect: Rect) -> Rect {
        let mapped = self.map_rect(rect);
        self.limit()
            .map_or(mapped, |limit| intersect(mapped, limit))
    }

    /// `rect` trimmed to the region being repainted: the pixels outside it are
    /// not copied back, so a rectangle fill need not rasterise them. Exact
    /// for an axis-aligned integer rectangle, whatever shader fills it.
    pub(super) fn scissored(&self, rect: Rect) -> Rect {
        self.scissor
            .map_or(rect, |scissor| intersect(rect, scissor))
    }

    /// The limit, trimmed to the region being repainted, for the text
    /// painters, which write pixels directly.
    pub(super) fn clip_bounds(&self) -> Option<Rect> {
        match (self.limit(), self.scissor) {
            (Some(limit), Some(scissor)) => Some(intersect(limit, scissor)),
            (None, scissor) => scissor,
            (limit, None) => limit,
        }
    }

    pub(super) fn corners(&self, corners: [Corner; 4]) -> [Corner; 4] {
        corners.map(|corner| Corner::new(corner.x * self.scale, corner.y * self.scale))
    }

    /// Builds a coverage mask for the active clips: the limit's rectangle,
    /// times every rounded clip's outline coverage. (Each rectangular clip's
    /// stored bounds already include the ones outside it, so the limit is all
    /// of them.) Only what the limit and the outlines can reach is written, so
    /// a small clip costs its own size, not the surface's.
    fn build_mask(&self) -> Option<Mask> {
        let (width, height) = (self.pixmap.width(), self.pixmap.height());
        let surface = Rect::new(0, 0, width as i32, height as i32);
        let mut mask = Mask::new(width, height)?;
        let Some(limit) = self.limit() else {
            // Nothing limits the pixels.
            mask.invert();
            return Some(mask);
        };
        // A limit only matters where the surface is, so it is trimmed to it:
        // that changes no pixel, and keeps one as large as the integer range
        // from being dropped by the rasteriser as too big for its arithmetic
        // (which would cut everything away).
        let bounds = intersect(limit, surface);
        let Some(path) = rect_path(bounds) else {
            // Nothing is inside the limit.
            return Some(mask);
        };
        mask.fill_path(&path, FILL_RULE, true, Transform::identity());
        // The rectangle outside which the mask is zero.
        let mut reach = bounds;
        for clip in &self.clips {
            let Clip::Rounded(..) = clip else {
                continue;
            };
            let Some(path) = clip.path() else {
                continue;
            };
            let outline = mask_reach(&path, width, height);
            let mut next = Mask::new(width, height)?;
            next.fill_path(&path, FILL_RULE, true, Transform::identity());
            multiply(&mut mask, &next, reach, outline);
            reach = intersect(reach, outline);
        }
        Some(mask)
    }

    /// Rebuilds the clip mask after a push/pop. A rounded clip always needs
    /// one; a purely rectangular clip is trimmed by intersecting each shape's
    /// rectangle, which is exact for rectangles and cheaper, so it waits for
    /// [`SkiaCanvas::clip_mask_for`] rather than allocating a mask per push.
    pub(super) fn rebuild_mask(&mut self) {
        let rounded = self
            .clips
            .iter()
            .any(|clip| matches!(clip, Clip::Rounded(..)));
        self.mask = if rounded { self.build_mask() } else { None };
    }

    /// Makes sure a mask exists for a shape under a clip, as the paths, ellipses
    /// and polygons always have: with an ancestor clip active they are cut by
    /// the mask whether or not they leave it. (A shape that leaves the limit
    /// gets one anyway, in [`SkiaCanvas::fill`] and [`SkiaCanvas::stroke`].)
    pub(super) fn ensure_mask(&mut self) {
        if self.mask.is_none() && !self.clips.is_empty() {
            self.mask = self.build_mask();
        }
    }

    /// Makes sure a shape with these device-space `bounds`, `grow` pixels wider
    /// for a stroke, is cut by the active clips: when it reaches outside the
    /// innermost clip a mask is built (a rounded clip already has one). A shape
    /// inside the clip needs no mask, so a painter that stays in its clip pays
    /// nothing.
    ///
    /// Every shape goes through [`SkiaCanvas::fill`] or [`SkiaCanvas::stroke`],
    /// which call this, so no outline, line or glow can leave a rectangular
    /// clip. (The rectangle fills trim their geometry to the clip instead,
    /// which a stroke cannot: its outline is wider than its path.)
    fn clip_mask_for(&mut self, bounds: tiny_skia::Rect, grow: f32) {
        if self.mask.is_none() && self.reaches_outside_clip(bounds, grow) {
            self.mask = self.build_mask();
        }
    }

    /// Whether a shape with these device-space `bounds`, `grow` pixels wider,
    /// lies partly outside the innermost clip.
    fn reaches_outside_clip(&self, bounds: tiny_skia::Rect, grow: f32) -> bool {
        let Some(clip) = self.limit() else {
            return false;
        };
        let shape = Rect::new(
            (bounds.left() - grow).floor() as i32,
            (bounds.top() - grow).floor() as i32,
            (bounds.right() + grow).ceil() as i32,
            (bounds.bottom() + grow).ceil() as i32,
        );
        shape.left < clip.left
            || shape.top < clip.top
            || shape.right > clip.right
            || shape.bottom > clip.bottom
    }

    /// Fills `path`, whose coordinates are already in device space (the
    /// transform is applied by [`SkiaCanvas::rect`]/[`SkiaCanvas::point`]),
    /// cut by the active clips.
    pub(super) fn fill(&mut self, path: &Path, shader: Shader<'static>) {
        let paint = paint(shader);
        self.clip_mask_for(path.bounds(), 0.0);
        let mask = self.mask.as_ref();
        self.pixmap
            .fill_path(path, &paint, FILL_RULE, Transform::identity(), mask);
    }

    pub(super) fn stroke(&mut self, path: &Path, color: tiny_skia::Color, stroke: &Stroke) {
        let paint = paint(solid_shader(color));
        let stroke = skia_stroke(stroke, self.scale);
        // The outline can lie a miter length from the path. Only a path near
        // the clip's edge is worth the outline's own bounds, which are what
        // decide whether it leaves the clip.
        let loose = stroke.width * stroke.miter_limit.max(1.0) + 1.0;
        if self.mask.is_none() && self.reaches_outside_clip(path.bounds(), loose) {
            let outline = path
                .stroke(&stroke, 1.0)
                .map_or(path.bounds(), |outline| outline.bounds());
            // The outline's bounds are already exact: no pixel outside the
            // whole pixels around them gets coverage, so a line that ends on
            // the clip's edge needs no mask (which would change its rounding).
            self.clip_mask_for(outline, 0.0);
        }
        let mask = self.mask.as_ref();
        self.pixmap
            .stroke_path(path, &paint, &stroke, Transform::identity(), mask);
    }

    /// The ellipse path at `center` with radii `radius_x`/`radius_y`, culled
    /// against the innermost clip.
    pub(super) fn ellipse_path(&self, center: Point, radius_x: f32, radius_y: f32) -> Option<Path> {
        let center = self.point(center);
        let bounds = Rect::new(
            (center.x as f32 - radius_x * self.scale).round() as i32,
            (center.y as f32 - radius_y * self.scale).round() as i32,
            (center.x as f32 + radius_x * self.scale).round() as i32,
            (center.y as f32 + radius_y * self.scale).round() as i32,
        );
        if bounds.is_empty() {
            return None;
        }
        let mut builder = PathBuilder::new();
        builder.push_oval(sk_rect(bounds));
        builder.finish()
    }
}

/// The whole-pixel rectangle, inside a `width` x `height` surface, that a clip
/// outline can give coverage to; one pixel wider than its bounds so that
/// anti-aliasing at the edge is inside it.
fn mask_reach(path: &Path, width: u32, height: u32) -> Rect {
    let bounds = path.bounds();
    let reach = Rect::new(
        (bounds.left().floor() as i32).saturating_sub(1),
        (bounds.top().floor() as i32).saturating_sub(1),
        (bounds.right().ceil() as i32).saturating_add(1),
        (bounds.bottom().ceil() as i32).saturating_add(1),
    );
    intersect(reach, Rect::new(0, 0, width as i32, height as i32))
}

/// tiny-skia's 8-bit product of two coverages, `a * b / 255` rounded, which
/// `Mask::intersect_path` applies; reproduced here so a mask built over the
/// rectangles the outlines reach is bit-for-bit what intersecting whole
/// surface masks gives.
fn coverage_product(a: u8, b: u8) -> u8 {
    let product = u32::from(a) * u32::from(b) + 128;
    ((product + (product >> 8)) >> 8) as u8
}

/// Multiplies `mask` by `next` (the coverage of one more clip outline) over
/// `held`, the rectangle `mask` can be non-zero in; `reach` is the one `next`
/// can be non-zero in, so everything else in `held` becomes zero.
fn multiply(mask: &mut Mask, next: &Mask, held: Rect, reach: Rect) {
    let width = mask.width() as usize;
    for y in held.top.max(0)..held.bottom {
        for x in held.left.max(0)..held.right {
            let at = y as usize * width + x as usize;
            let inside = x >= reach.left && x < reach.right && y >= reach.top && y < reach.bottom;
            let value = if inside {
                coverage_product(mask.data()[at], next.data()[at])
            } else {
                0
            };
            mask.data_mut()[at] = value;
        }
    }
}

/// The coverage mask text and images are cut by: only a rounded clip's. A
/// rectangular clip is applied to them by trimming their bounds, which is exact
/// and keeps the faster paths (an unscaled image is a row copy); the mask a
/// shape crossing a rectangular clip built is for paths.
pub(super) fn pixel_mask<'a>(clips: &[Clip], mask: &'a Option<Mask>) -> Option<&'a Mask> {
    let rounded = clips.iter().any(|clip| matches!(clip, Clip::Rounded(..)));
    if rounded { mask.as_ref() } else { None }
}
