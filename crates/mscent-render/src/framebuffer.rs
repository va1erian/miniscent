//! Offscreen colour + depth target.
//!
//! Colour is `0xAARRGGBB` (alpha in the top byte, which the renderer keeps at
//! 255). Depth stores `1/z`, so 0.0 means "nothing drawn" and larger is
//! nearer; this makes the per-pixel depth test a single compare.

use crate::camera::MAX_SIDE;
use crate::shade::lerp_rgb;

/// Colour and depth buffers of one render target.
#[derive(Clone, Debug)]
pub struct Framebuffer {
    width: u32,
    height: u32,
    color: Vec<u32>,
    depth: Vec<f32>,
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

    /// Colour and depth planes together, for compositing tiles and for a
    /// renderer that draws elsewhere and copies its frame here.
    pub fn planes_mut(&mut self) -> (&mut [u32], &mut [f32]) {
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
        let last = (self.full_height - 1).max(1);
        for (y, row) in self.color.chunks_exact_mut(w).enumerate() {
            // Screen row, so tiles of one target share a single gradient.
            let t = ((self.y0 + y as u32) * 256 / last).min(256);
            row.fill(lerp_rgb(top, bottom, t));
        }
        self.depth.fill(0.0);
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
