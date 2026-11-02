//! Per-pixel lightmap fetches: bilinear within a level, blended between two
//! levels (trilinear) where a pixel covers more than a luxel.
//!
//! Every fetch is clamped to the face's own grid, so neighbouring faces in
//! the atlas never bleed in. The level choice is made once per span
//! ([`FaceSampler::plan`]); consecutive pixels mostly land in the same 2x2
//! cell of luxels, so the cell's bilinear coefficients are cached and a pixel
//! costs a few vector multiply-adds.

use mscent_math::Vec3;
use mscent_world::lightmap::FaceGrid;

use crate::lightmap_mips::{ColorGrid, Rgbx};
use crate::raster::SPAN_LEN;

/// Most pixels one call to [`Plan::colors`] may fill.
pub const MAX_RUN: usize = SPAN_LEN as usize;

/// Below this level fraction the finer level is used alone.
pub const MIN_BLEND: f32 = 1.0 / 64.0;

fn zip(a: Rgbx, b: Rgbx, f: impl Fn(f32, f32) -> f32) -> Rgbx {
    std::array::from_fn(|k| f(a[k], b[k]))
}

/// The 2x2 cell last fetched at one level, as bilinear coefficients: the
/// colour at `(tx, ty)` is `c00 + tx (c10 - c00) + ty (c01 - c00) + tx ty
/// (c00 - c10 - c01 + c11)`.
#[derive(Clone, Copy, Debug)]
pub struct Cell {
    /// Level and position of the cell; `u64::MAX` before the first fetch.
    key: u64,
    coef: [Rgbx; 4],
}

impl Cell {
    pub const EMPTY: Cell = Cell {
        key: u64::MAX,
        coef: [[0.0; 4]; 4],
    };

    fn key_of(level: usize, x: u32, y: u32) -> u64 {
        (level as u64) << 56 | u64::from(x) << 28 | u64::from(y)
    }
}

/// One face's levels: the colour grids, and the baked grid for directions.
#[derive(Clone, Copy)]
pub struct FaceSampler<'a> {
    base: FaceGrid<'a>,
    levels: &'a [ColorGrid],
}

/// One level, resolved for a run of pixels.
#[derive(Clone, Copy)]
struct Pass<'a> {
    grid: Option<&'a ColorGrid>,
    level: usize,
    /// Luxel coordinates of the baked grid to coordinates of this level.
    scale: f32,
    max_x: f32,
    max_y: f32,
}

/// The levels one span samples, and the blend between them.
#[derive(Clone, Copy)]
pub struct Plan<'a> {
    fine: Pass<'a>,
    coarse: Option<(Pass<'a>, f32)>,
}

impl<'a> FaceSampler<'a> {
    /// `levels` are the colour grids of the face, finest first.
    pub fn new(base: FaceGrid<'a>, levels: &'a [ColorGrid]) -> Self {
        Self { base, levels }
    }

    fn pass(&self, level: usize) -> Pass<'a> {
        let grid = self.levels.get(level);
        Pass {
            grid,
            level,
            // 2^-level, exactly.
            scale: f32::from_bits(127u32.saturating_sub(level as u32) << 23),
            max_x: grid.map_or(0.0, |g| g.width().saturating_sub(1) as f32),
            max_y: grid.map_or(0.0, |g| g.height().saturating_sub(1) as f32),
        }
    }

    /// Levels to sample at level of detail `lod` (0 = baked grid, 1 = first
    /// mip, ...), clamped to the chain; NaN reads level 0.
    pub fn plan(&self, lod: f32) -> Plan<'a> {
        let top = self.levels.len().saturating_sub(1);
        let lod = if lod.is_nan() {
            0.0
        } else {
            lod.clamp(0.0, top as f32)
        };
        let l0 = lod as usize;
        let frac = lod - l0 as f32;
        let coarse = (frac >= MIN_BLEND && l0 < top).then(|| (self.pass(l0 + 1), frac));
        Plan {
            fine: self.pass(l0),
            coarse,
        }
    }

    /// Bilinear sum of the baked light directions around luxel coordinates
    /// `(lx, ly)`; a weighted sum of unit vectors, not normalised.
    pub fn dir(&self, lx: f32, ly: f32) -> Vec3 {
        let g = &self.base;
        // NaN lands on luxel 0 (`max` ignores it; `clamp` would not).
        let clamp = |c: f32, n: u32| (c - 0.5).max(0.0).min(n.saturating_sub(1) as f32);
        let (fx, fy) = (clamp(lx, g.width()), clamp(ly, g.height()));
        let (x0, y0) = (fx as u32, fy as u32);
        let (tx, ty) = (fx - x0 as f32, fy - y0 as f32);
        let d = |x: u32, y: u32| g.at_clamped(x, y).dir;
        let top = d(x0, y0) * (1.0 - tx) + d(x0 + 1, y0) * tx;
        let bottom = d(x0, y0 + 1) * (1.0 - tx) + d(x0 + 1, y0 + 1) * tx;
        top * (1.0 - ty) + bottom * ty
    }
}

impl Pass<'_> {
    /// Make `cell` hold the coefficients of the 2x2 luxels at `(x0, y0)`.
    #[inline(always)]
    fn fill(&self, x0: u32, y0: u32, cell: &mut Cell) {
        let key = Cell::key_of(self.level, x0, y0);
        if cell.key != key {
            let [c00, c10, c01, c11] = self.grid.map_or([[0.0; 4]; 4], |g| g.cell(x0, y0));
            let d10 = zip(c10, c00, |a, b| a - b);
            let d01 = zip(c01, c00, |a, b| a - b);
            let d11 = zip(c11, c01, |a, b| a - b);
            *cell = Cell {
                key,
                coef: [c00, d10, d01, zip(d11, d10, |a, b| a - b)],
            };
        }
    }

    /// Colour of `out.len()` consecutive pixels, times `gain`, starting at
    /// luxel `(lx, ly)` of the baked grid and moving `(dlx, dly)` per pixel.
    fn pixels(
        &self,
        (lx, ly): (f32, f32),
        (dlx, dly): (f32, f32),
        gain: f32,
        cell: &mut Cell,
        out: &mut [Rgbx],
    ) {
        for (i, o) in out.iter_mut().enumerate() {
            // `max` before `min` so a NaN coordinate lands on the first luxel
            // (`clamp` would pass it through and poison the colour).
            let fx = ((lx + dlx * i as f32) * self.scale - 0.5)
                .max(0.0)
                .min(self.max_x);
            let fy = ((ly + dly * i as f32) * self.scale - 0.5)
                .max(0.0)
                .min(self.max_y);
            let (x0, y0) = (fx as u32, fy as u32);
            self.fill(x0, y0, cell);
            let (tx, ty) = (fx - x0 as f32, fy - y0 as f32);
            let k = &cell.coef;
            *o = std::array::from_fn(|c| {
                (k[0][c] + (k[1][c] + k[3][c] * ty) * tx + k[2][c] * ty) * gain
            });
        }
    }
}

impl Plan<'_> {
    /// Light colour times `gain` of `out.len()` consecutive pixels (at most
    /// [`MAX_RUN`]) starting at luxel coordinates `start` of the baked grid
    /// and moving `step` per pixel.
    pub fn colors(
        &self,
        start: (f32, f32),
        step: (f32, f32),
        gain: f32,
        cells: &mut [Cell; 2],
        out: &mut [Rgbx],
    ) {
        self.fine.pixels(start, step, gain, &mut cells[0], out);
        if let Some((pass, frac)) = &self.coarse {
            let mut far = [[0.0; 4]; MAX_RUN];
            let far = &mut far[..out.len().min(MAX_RUN)];
            pass.pixels(start, step, gain, &mut cells[1], far);
            for (o, f) in out.iter_mut().zip(far.iter()) {
                *o = zip(*o, *f, |a, b| a + (b - a) * frac);
            }
        }
    }
}

#[cfg(test)]
#[path = "light_sampler_tests.rs"]
mod light_sampler_tests;
