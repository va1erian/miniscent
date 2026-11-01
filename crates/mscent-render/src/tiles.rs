//! Tile-parallel rendering.
//!
//! The target is cut into horizontal bands of `tile_rows` rows. Workers pull
//! bands from a shared queue, run the caller's draw function on each band and
//! copy the finished band into the target. A band is a [`Framebuffer`] that
//! knows which screen rows it holds, and the rasterizer clips every polygon to
//! those rows, so a band produces exactly the pixels the full frame would
//! have in the same rows: the output does not depend on the worker count or
//! on which worker took which band.
//!
//! One worker is a valid setting and runs everything on the calling thread
//! without spawning. The band buffers are allocated once, in
//! [`TileRenderer::new`]; a frame allocates nothing and costs one thread spawn
//! per extra worker (scoped threads, so borrowed scene data needs no `Arc`).

use std::sync::{Mutex, PoisonError};
use std::thread;

use crate::framebuffer::Framebuffer;

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
    tiles: Vec<Framebuffer>,
}

impl TileRenderer {
    /// `None` when the size is invalid (see [`Framebuffer::new`]) or
    /// `tile_rows` is zero. `workers` is clamped to `1..=MAX_WORKERS`.
    pub fn new(width: u32, height: u32, tile_rows: u32, workers: usize) -> Option<Self> {
        if tile_rows == 0 {
            return None;
        }
        // Validates the size up front so the band loop cannot fail midway.
        Framebuffer::new(width, height)?;
        let mut tiles = Vec::new();
        let mut y0 = 0;
        while y0 < height {
            let rows = tile_rows.min(height - y0);
            tiles.push(Framebuffer::new_band(width, y0, rows, height)?);
            y0 += rows;
        }
        Some(Self {
            width,
            height,
            tile_rows,
            workers: workers.clamp(1, MAX_WORKERS),
            tiles,
        })
    }

    pub fn workers(&self) -> usize {
        self.workers
    }

    /// Change the worker count, clamped to `1..=MAX_WORKERS`.
    pub fn set_workers(&mut self, workers: usize) {
        self.workers = workers.clamp(1, MAX_WORKERS);
    }

    pub fn tile_count(&self) -> usize {
        self.tiles.len()
    }

    /// Run `draw` once per band, in parallel, then store the bands in
    /// `target`. `draw` must clear the band itself (for example with
    /// [`Framebuffer::clear_sky`]) and draw everything that should appear in
    /// it; it sees only the band's rows but uses screen coordinates.
    pub fn render<F>(&mut self, target: &mut Framebuffer, draw: &F) -> Result<(), SizeMismatch>
    where
        F: Fn(&mut Framebuffer) + Sync,
    {
        if target.width() != self.width || target.height() != self.height {
            return Err(SizeMismatch);
        }
        let helpers = self.workers.min(self.tiles.len()).saturating_sub(1);
        let chunk = self.width as usize * self.tile_rows as usize;
        let (colors, depths) = target.planes_mut();
        // The queue hands out disjoint (band, colour rows, depth rows)
        // triples, so workers never touch the same memory.
        let queue = Mutex::new(
            self.tiles
                .iter_mut()
                .zip(colors.chunks_mut(chunk))
                .zip(depths.chunks_mut(chunk)),
        );
        let work = || {
            loop {
                let next = queue.lock().unwrap_or_else(PoisonError::into_inner).next();
                let Some(((tile, color), depth)) = next else {
                    break;
                };
                draw(tile);
                let (tc, td) = tile.planes_mut();
                color.copy_from_slice(tc);
                depth.copy_from_slice(td);
            }
        };
        if helpers == 0 {
            work();
        } else {
            thread::scope(|s| {
                for _ in 0..helpers {
                    s.spawn(work);
                }
                work();
            });
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "tiles_tests.rs"]
mod tiles_tests;
