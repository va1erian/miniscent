//! Offscreen colour + depth target.
//!
//! Colour is `0xAARRGGBB` (alpha in the top byte, which the renderer keeps at
//! 255). Depth stores `1/z`, so 0.0 means "nothing drawn" and larger is
//! nearer; this makes the per-pixel depth test a single compare.

use crate::camera::MAX_SIDE;
use crate::shade::lerp_rgb;

/// The colour a viewport face of `slot` paints. Alpha 0 can come from nothing
/// else, and no such pixel outlasts the pass that made it.
pub(crate) const fn hole_mark(slot: u32) -> u32 {
    slot
}

/// The gradient's colour at a screen row of a target `full_height` tall.
fn sky_at(row: u32, full_height: u32, top: u32, bottom: u32) -> u32 {
    let last = (full_height - 1).max(1);
    lerp_rgb(top, bottom, (row * 256 / last).min(256))
}

/// Colour and depth buffers of one render target.
#[derive(Clone, Debug)]
pub struct Framebuffer {
    width: u32,
    height: u32,
    color: Vec<u32>,
    depth: Vec<f32>,
    /// Depth of a vista while one draws; empty until the first does.
    view_depth: Vec<f32>,
    /// First screen row held (non-zero only for a tile of a larger target).
    y0: u32,
    /// Height of the whole target this buffer is a part of.
    full_height: u32,
}

impl Framebuffer {
    /// `None` when a side is zero or above 8192.
    pub fn new(width: u32, height: u32) -> Option<Self> {
        let ok = |s: u32| (1..=MAX_SIDE).contains(&s);
        if !ok(width) || !ok(height) {
            return None;
        }
        let n = width as usize * height as usize;
        Some(Self {
            width,
            height,
            color: vec![0xff00_0000; n],
            depth: vec![0.0; n],
            view_depth: Vec::new(),
            y0: 0,
            full_height: height,
        })
    }

    /// A horizontal band: `rows` rows starting at screen row `y0` of a
    /// `full_height` target. Drawing and reads use screen coordinates, so a
    /// band renders exactly the pixels the full target would have in its rows.
    pub(crate) fn new_band(width: u32, y0: u32, rows: u32, full_height: u32) -> Option<Self> {
        let end = y0.checked_add(rows)?;
        if end > full_height {
            return None;
        }
        let mut band = Self::new(width, rows)?;
        band.y0 = y0;
        band.full_height = full_height;
        Some(band)
    }

    /// Remake this buffer as another band, reusing its storage: nothing is
    /// allocated unless the new band is larger than any it has held. Colour
    /// and depth are left as they were, for a caller that writes what it
    /// reads. `None`, leaving the buffer as it was, for a band
    /// [`Self::new_band`] would refuse.
    pub(crate) fn reshape_band(
        &mut self,
        width: u32,
        y0: u32,
        rows: u32,
        full_height: u32,
    ) -> Option<()> {
        let ok = |s: u32| (1..=MAX_SIDE).contains(&s);
        if !ok(width) || !ok(rows) || y0.checked_add(rows)? > full_height {
            return None;
        }
        let n = width as usize * rows as usize;
        self.color.resize(n, 0xff00_0000);
        self.depth.resize(n, 0.0);
        (self.width, self.height) = (width, rows);
        (self.y0, self.full_height) = (y0, full_height);
        Some(())
    }

    /// Height of the whole target this buffer is a part of.
    pub(crate) fn full_height(&self) -> u32 {
        self.full_height
    }

    /// Stored `1/z` of screen row `y`, if this buffer holds it.
    pub(crate) fn depth_row(&self, y: i32) -> Option<&[f32]> {
        let y = usize::try_from(y)
            .ok()?
            .checked_sub(self.y0 as usize)
            .filter(|&y| y < self.height as usize)?;
        let w = self.width as usize;
        self.depth.get(y * w..(y + 1) * w)
    }

    /// Screen rows `start..end` this buffer holds.
    pub(crate) fn row_range(&self) -> (i32, i32) {
        (self.y0 as i32, (self.y0 + self.height) as i32)
    }

    /// Colour and depth planes together, for compositing tiles.
    pub(crate) fn planes_mut(&mut self) -> (&mut [u32], &mut [f32]) {
        (&mut self.color, &mut self.depth)
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    /// Row-major pixels, `width * height` of them.
    pub fn pixels(&self) -> &[u32] {
        &self.color
    }

    /// Mutable row-major pixels, for 2D drawing and presentation code.
    pub fn pixels_mut(&mut self) -> &mut [u32] {
        &mut self.color
    }

    /// Fill colour and reset depth.
    pub fn clear(&mut self, color: u32) {
        self.color.fill(color);
        self.depth.fill(0.0);
    }

    /// Fill the colour with a vertical gradient (the sky) and reset depth.
    pub fn clear_sky(&mut self, top: u32, bottom: u32) {
        let w = self.width as usize;
        let (y0, full) = (self.y0, self.full_height);
        for (y, row) in self.color.chunks_exact_mut(w).enumerate() {
            // Screen row, so tiles of one target share a single gradient.
            row.fill(sky_at(y0 + y as u32, full, top, bottom));
        }
        self.depth.fill(0.0);
    }

    /// Starts drawing a vista through the marks of `slot`: those pixels take the
    /// gradient, so what no layer covers fails visibly, and every other pixel
    /// refuses all depth tests until [`Self::close_view`]. The scene's depth is
    /// kept for what draws after the vista. The second plane is made on first
    /// use, so a level without viewports never pays for it.
    pub(crate) fn open_view(&mut self, slot: u32, top: u32, bottom: u32) {
        let mark = hole_mark(slot);
        self.view_depth.resize(self.depth.len(), 0.0);
        let w = self.width as usize;
        let rows = self.height;
        for y in 0..rows {
            let sky = sky_at(self.y0 + y, self.full_height, top, bottom);
            let start = y as usize * w;
            let color = &mut self.color[start..start + w];
            let depth = &mut self.view_depth[start..start + w];
            for (c, d) in color.iter_mut().zip(depth) {
                if *c == mark {
                    *c = sky;
                    *d = 0.0;
                } else {
                    *d = f32::INFINITY;
                }
            }
        }
        core::mem::swap(&mut self.depth, &mut self.view_depth);
    }

    /// Between two layers of a vista: the mask stays and the depth is forgotten,
    /// so the next layer draws over the last wherever it covers.
    pub(crate) fn next_view_layer(&mut self) {
        for d in self.depth.iter_mut().filter(|d| d.is_finite()) {
            *d = 0.0;
        }
    }

    /// Puts the scene's depth back after [`Self::open_view`].
    pub(crate) fn close_view(&mut self) {
        core::mem::swap(&mut self.depth, &mut self.view_depth);
    }

    fn index(&self, x: i32, y: i32) -> Option<usize> {
        let (x, y) = (u32::try_from(x).ok()?, u32::try_from(y).ok()?);
        let y = y.checked_sub(self.y0)?;
        (x < self.width && y < self.height).then(|| y as usize * self.width as usize + x as usize)
    }

    pub fn pixel(&self, x: i32, y: i32) -> Option<u32> {
        self.index(x, y).map(|i| self.color[i])
    }

    /// Stored `1/z` at a pixel; 0.0 where nothing was drawn.
    pub fn depth_at(&self, x: i32, y: i32) -> Option<f32> {
        self.index(x, y).map(|i| self.depth[i])
    }

    /// Mutable colour and depth of row `y`, or `None` when out of range.
    pub(crate) fn row_mut(&mut self, y: i32) -> Option<(&mut [u32], &mut [f32])> {
        let y = usize::try_from(y)
            .ok()?
            .checked_sub(self.y0 as usize)
            .filter(|&y| y < self.height as usize)?;
        let w = self.width as usize;
        Some((
            &mut self.color[y * w..(y + 1) * w],
            &mut self.depth[y * w..(y + 1) * w],
        ))
    }
}

#[cfg(test)]
#[path = "framebuffer_tests.rs"]
mod framebuffer_tests;
