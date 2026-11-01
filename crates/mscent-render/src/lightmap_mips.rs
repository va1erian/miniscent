//! Per-face colour grids and their box-filtered mip levels.
//!
//! Far away, one screen pixel spans several luxels; sampling the full-size
//! grid there makes shadow edges shimmer. Every face keeps its own chain,
//! halved from its own grid with the edge luxels clamped, so no level ever
//! mixes two faces of the atlas.
//!
//! The grids are laid out for the per-pixel fetch: one `[f32; 4]` per luxel
//! (alpha unused, so a fetch is one vector load), with an extra column and
//! row repeating the last luxel, so the 2x2 neighbourhood of any cell is in
//! bounds without clamping. Level 0 is the baked colour itself.

use mscent_world::lightmap::{FaceGrid, Lightmap};

/// Most levels kept below the base (a 64-luxel footprint is far past any
/// face the baker produces).
const MAX_LEVELS: usize = 6;

/// One colour (and a spare lane) per luxel.
pub type Rgbx = [f32; 4];

/// Colours of one level of one face.
#[derive(Clone, Debug)]
pub struct ColorGrid {
    w: u32,
    h: u32,
    /// `(w + 1) * (h + 1)` entries, the last column and row repeated.
    texels: Vec<Rgbx>,
}

impl ColorGrid {
    fn from_fn(w: u32, h: u32, at: impl Fn(u32, u32) -> Rgbx) -> Self {
        let mut texels = Vec::with_capacity((w as usize + 1) * (h as usize + 1));
        for y in 0..=h {
            for x in 0..=w {
                texels.push(at(x.min(w.saturating_sub(1)), y.min(h.saturating_sub(1))));
            }
        }
        Self { w, h, texels }
    }

    pub fn width(&self) -> u32 {
        self.w
    }

    pub fn height(&self) -> u32 {
        self.h
    }

    /// The luxels at `(x, y)`, `(x + 1, y)`, `(x, y + 1)` and `(x + 1, y + 1)`;
    /// black outside the grid.
    #[inline(always)]
    pub fn cell(&self, x: u32, y: u32) -> [Rgbx; 4] {
        let stride = self.w as usize + 1;
        let i = y as usize * stride + x as usize;
        let row = |at: usize| match self.texels.get(at..at + 2) {
            Some(&[a, b]) => [a, b],
            _ => [[0.0; 4]; 2],
        };
        let (top, bottom) = (row(i), row(i + stride));
        [top[0], top[1], bottom[0], bottom[1]]
    }

    fn at(&self, x: u32, y: u32) -> Rgbx {
        self.cell(
            x.min(self.w.saturating_sub(1)),
            y.min(self.h.saturating_sub(1)),
        )[0]
    }

    /// Half-size grid: each luxel is the mean of the up-to-2x2 luxels under it.
    fn halved(&self) -> Self {
        Self::from_fn(self.w.div_ceil(2), self.h.div_ceil(2), |x, y| {
            let mut sum = [0.0f32; 4];
            let mut n = 0.0;
            for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                let (sx, sy) = (x * 2 + dx, y * 2 + dy);
                // Luxels past an odd edge do not exist: skip them rather
                // than counting the edge luxel twice.
                if sx < self.w && sy < self.h {
                    let c = self.at(sx, sy);
                    sum = std::array::from_fn(|k| sum[k] + c[k]);
                    n += 1.0;
                }
            }
            sum.map(|s| s / f32::max(n, 1.0))
        })
    }
}

/// Colour grids of every face of a lightmap: level 0 (the baked colours)
/// and the mips below it.
#[derive(Clone, Debug, Default)]
pub struct LightMips {
    faces: Vec<Vec<ColorGrid>>,
}

impl LightMips {
    /// Copy and filter every face of `lm`.
    pub fn build(lm: &Lightmap) -> Self {
        let faces = (0..lm.projections().len())
            .map(|f| lm.face_grid(f).map_or_else(Vec::new, |g| chain(&g)))
            .collect();
        Self { faces }
    }

    /// Levels of `face`, finest first (empty for an unknown face).
    pub fn levels(&self, face: usize) -> &[ColorGrid] {
        self.faces.get(face).map_or(&[], Vec::as_slice)
    }
}

fn chain(grid: &FaceGrid<'_>) -> Vec<ColorGrid> {
    let (w, h) = (grid.width(), grid.height());
    let base = ColorGrid::from_fn(w, h, |x, y| {
        let c = grid.at_clamped(x, y).color;
        [c.x, c.y, c.z, 0.0]
    });
    let mut out = vec![base];
    while out.len() <= MAX_LEVELS {
        let Some(last) = out.last() else { break };
        if last.w <= 1 && last.h <= 1 {
            break;
        }
        let next = last.halved();
        out.push(next);
    }
    out
}

#[cfg(test)]
#[path = "lightmap_mips_tests.rs"]
mod lightmap_mips_tests;
