//! Tile-parallel rendering.
//!
//! The target is cut into horizontal bands of `tile_rows` rows. A frame is one
//! or more passes: workers pull bands from a shared queue and run the caller's
//! draw function on each, and [`TileRenderer::composite`] then copies the
//! finished bands into the target. A band is a [`Framebuffer`] that knows which
//! screen rows it holds, and the rasterizer clips every polygon to those rows,
//! so a band produces exactly the pixels the full frame would have in the same
//! rows: the output does not depend on the worker count or on which worker took
//! which band.
//!
//! One worker is a valid setting and runs everything on the calling thread.
//! The band buffers and the worker threads are made up front; a frame allocates
//! nothing here and only wakes the threads ([`JobPool`]).

use std::sync::{Mutex, PoisonError};

use crate::framebuffer::Framebuffer;
use crate::jobs::JobPool;

/// Upper bound on workers; more threads than bands would only idle.
pub const MAX_WORKERS: usize = 64;

/// The target given to [`TileRenderer::render`] differs in size from the one
/// the renderer was built for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SizeMismatch;

/// Renders a fixed-size target in parallel bands.
#[derive(Debug)]
pub struct TileRenderer {
    width: u32,
    height: u32,
    tile_rows: u32,
    workers: usize,
    pool: JobPool,
    tiles: Vec<Framebuffer>,
}

/// The bands of a target, or `None` when the size is invalid.
fn bands(width: u32, height: u32, tile_rows: u32) -> Option<Vec<Framebuffer>> {
    // Validates the size up front so the band loop cannot fail midway.
    Framebuffer::new(width, height)?;
    let mut tiles = Vec::new();
    let mut y0 = 0;
    while y0 < height {
        let rows = tile_rows.min(height - y0);
        tiles.push(Framebuffer::new_band(width, y0, rows, height)?);
        y0 += rows;
    }
    Some(tiles)
}

impl TileRenderer {
    /// `None` when the size is invalid (see [`Framebuffer::new`]) or
    /// `tile_rows` is zero. `workers` is clamped to `1..=MAX_WORKERS`.
    pub fn new(width: u32, height: u32, tile_rows: u32, workers: usize) -> Option<Self> {
        if tile_rows == 0 {
            return None;
        }
        let tiles = bands(width, height, tile_rows)?;
        let workers = workers.clamp(1, MAX_WORKERS);
        Some(Self {
            width,
            height,
            tile_rows,
            workers,
            pool: JobPool::new(workers),
            tiles,
        })
    }

    /// Width and height of the target this renderer is for.
    pub fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    /// Change the target size, keeping the worker threads. `None` for an
    /// invalid size, which leaves the renderer as it was.
    pub fn resize(&mut self, width: u32, height: u32) -> Option<()> {
        self.tiles = bands(width, height, self.tile_rows)?;
        (self.width, self.height) = (width, height);
        Some(())
    }

    pub fn workers(&self) -> usize {
        self.workers
    }

    /// Change the worker count, clamped to `1..=MAX_WORKERS`.
    pub fn set_workers(&mut self, workers: usize) {
        self.workers = workers.clamp(1, MAX_WORKERS);
        self.pool = JobPool::new(self.workers);
    }

    pub fn tile_count(&self) -> usize {
        self.tiles.len()
    }

    /// One whole frame: [`Self::pass`] with `draw`, then [`Self::composite`].
    /// `draw` must clear the band itself (for example with
    /// [`Framebuffer::clear_sky`]) and draw everything that should appear in
    /// it; it sees only the band's rows but uses screen coordinates.
    pub fn render<F>(&mut self, target: &mut Framebuffer, draw: &F) -> Result<(), SizeMismatch>
    where
        F: Fn(&mut Framebuffer) + Sync,
    {
        self.check_size(target)?;
        self.pass(draw);
        self.composite(target)
    }

    /// Run `draw` once per band, in parallel. A band keeps what the last pass
    /// left in it until [`Self::composite`], so a later pass draws over it.
    pub fn pass<F>(&mut self, draw: &F)
    where
        F: Fn(&mut Framebuffer) + Sync,
    {
        let jobs = self.tiles.len();
        // The queue hands each worker a band of its own.
        let queue = Mutex::new(self.tiles.iter_mut());
        self.pool.run(jobs, &|_| {
            let next = queue.lock().unwrap_or_else(PoisonError::into_inner).next();
            if let Some(tile) = next {
                draw(tile);
            }
        });
    }

    /// Store the bands in `target`, colour and depth.
    pub fn composite(&mut self, target: &mut Framebuffer) -> Result<(), SizeMismatch> {
        self.check_size(target)?;
        let chunk = self.width as usize * self.tile_rows as usize;
        let (colors, depths) = target.planes_mut();
        let rows = colors.chunks_mut(chunk).zip(depths.chunks_mut(chunk));
        for (tile, (color, depth)) in self.tiles.iter_mut().zip(rows) {
            let (tc, td) = tile.planes_mut();
            color.copy_from_slice(tc);
            depth.copy_from_slice(td);
        }
        Ok(())
    }

    fn check_size(&self, target: &Framebuffer) -> Result<(), SizeMismatch> {
        if target.width() != self.width || target.height() != self.height {
            return Err(SizeMismatch);
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "tiles_tests.rs"]
mod tiles_tests;
