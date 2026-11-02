//! The half-resolution layer: big, soft sprites drawn into a buffer a quarter
//! the size and then composited over the band, so a large explosion costs about
//! a quarter of its fill and keeps its smoke.
//!
//! A band draws its own part, inside the one pass over the bands:
//!
//! 1. take the union of the layered sprites' boxes in layer pixels, and stop if
//!    it is empty, so a band with no big sprite in it pays nothing;
//! 2. fill the layer's depth from the band's, taking the *nearest* of each 2x2
//!    block, so a wall in front of a sprite is never painted over (the cost is
//!    a fringe of up to one full-size pixel where the sprite loses to it);
//! 3. draw the sprites at half size with the [`Dest::Layer`] kernels, which
//!    accumulate colour in RGB and the product of what each sprite lets
//!    through in the alpha byte;
//! 4. composite by nearest-neighbour upscale: `dst = layer.rgb + dst * keep`.
//!
//! A layer row stands for two screen rows, so a band can hold its rows only if
//! it starts on an even row and ends on an even row or at the end of the frame
//! ([`fits`]). Any other band draws the layered sprites at full size, so the
//! pixels of such a tiling differ from the layered ones, though never between
//! worker counts. Only the union of the sprites' boxes is prepared and
//! composited; outside it the layer is the identity and is never read.

use std::cell::RefCell;

use super::bank::SpriteBank;
use super::kernels::Dest;
use super::layer_kernels::{CLEAR, composite_row, nearest_row};
use super::sprite::{self, Bounds, MAX_RADIUS, draw_sprite};
use super::{Blend, ParticleDraws, SpriteDraw};
use crate::framebuffer::Framebuffer;
use crate::simd::Level;

thread_local! {
    /// This worker's layer, made once and reshaped in place after, so a frame
    /// allocates nothing as long as bands keep their size.
    static SCRATCH: RefCell<Option<Framebuffer>> = const { RefCell::new(None) };
}

/// Whether the band's rows pair up exactly into layer rows.
pub(super) fn fits(fb: &Framebuffer) -> bool {
    let (start, end) = fb.row_range();
    start % 2 == 0 && (end % 2 == 0 || end as u32 == fb.full_height())
}

/// Draw the band's layered sprites through the layer. `false` when this band
/// cannot, and the caller must draw them at full size. `true` with nothing done
/// when there is nothing to draw.
pub(super) fn draw(
    level: Level,
    fb: &mut Framebuffer,
    draws: &ParticleDraws,
    bank: &SpriteBank,
) -> bool {
    if draws.layer_over.is_empty() && draws.layer_add.is_empty() {
        return true;
    }
    if !fits(fb) {
        return false;
    }
    SCRATCH.with(|cell| {
        let Ok(mut slot) = cell.try_borrow_mut() else {
            return false;
        };
        let Some(layer) = shape(&mut slot, fb) else {
            return false;
        };
        let sprites = draws.layer_over.iter().chain(&draws.layer_add);
        let Some(region) = sprites
            .filter_map(|s| sprite::bounds(&half(s), layer))
            .reduce(union)
        else {
            return true;
        };
        prepare(level, fb, layer, region);
        for s in &draws.layer_over {
            draw_sprite(level, layer, &half(s), (Blend::Over, Dest::Layer), bank);
        }
        for s in &draws.layer_add {
            draw_sprite(level, layer, &half(s), (Blend::Add, Dest::Layer), bank);
        }
        composite(level, layer, fb, region);
        true
    })
}

/// The layer for `fb`'s rows, reusing the scratch.
fn shape<'a>(slot: &'a mut Option<Framebuffer>, fb: &Framebuffer) -> Option<&'a mut Framebuffer> {
    let (start, end) = fb.row_range();
    let half = |n: u32| n.div_ceil(2);
    let args = (
        half(fb.width()),
        start as u32 / 2,
        half((end - start) as u32),
        half(fb.full_height()),
    );
    match slot {
        Some(layer) => layer.reshape_band(args.0, args.1, args.2, args.3)?,
        None => *slot = Some(Framebuffer::new_band(args.0, args.1, args.2, args.3)?),
    }
    slot.as_mut()
}

/// A sprite in layer pixels. The radius is capped as a full-size sprite's is,
/// before halving; a NaN fails the comparison and is left for the clip to
/// refuse.
fn half(s: &SpriteDraw) -> SpriteDraw {
    let radius = if s.radius > MAX_RADIUS {
        MAX_RADIUS
    } else {
        s.radius
    };
    SpriteDraw {
        x: s.x * 0.5,
        y: s.y * 0.5,
        radius: radius * 0.5,
        ..*s
    }
}

fn union(a: Bounds, b: Bounds) -> Bounds {
    Bounds {
        cols: (a.cols.0.min(b.cols.0), a.cols.1.max(b.cols.1)),
        rows: (a.rows.0.min(b.rows.0), a.rows.1.max(b.rows.1)),
    }
}

/// Clear the layer and fill its depth over `region`.
fn prepare(level: Level, fb: &Framebuffer, layer: &mut Framebuffer, region: Bounds) {
    let (x0, x1) = region.cols;
    for j in region.rows.0..region.rows.1 {
        let top = fb.depth_row(2 * j as i32);
        // The bottom row of a frame of odd height is missing: its block is
        // the top row alone.
        let bottom = fb.depth_row(2 * j as i32 + 1).or(top);
        let (Some(top), Some(bottom), Some((color, depth))) =
            (top, bottom, layer.row_mut(j as i32))
        else {
            continue;
        };
        let span = 2 * x0..(2 * x1).min(top.len());
        if let (Some(top), Some(bottom), Some(out)) = (
            top.get(span.clone()),
            bottom.get(span),
            depth.get_mut(x0..x1),
        ) {
            nearest_row(level, out, top, bottom);
        }
        if let Some(c) = color.get_mut(x0..x1) {
            c.fill(CLEAR);
        }
    }
}

/// Upscale the layer's `region` over `fb`, two screen rows per layer row.
fn composite(level: Level, layer: &mut Framebuffer, fb: &mut Framebuffer, region: Bounds) {
    let (x0, x1) = region.cols;
    for j in region.rows.0..region.rows.1 {
        let Some((px, _)) = layer.row_mut(j as i32) else {
            continue;
        };
        let Some(px) = px.get(x0..x1) else { continue };
        for y in [2 * j as i32, 2 * j as i32 + 1] {
            let Some((color, _)) = fb.row_mut(y) else {
                continue;
            };
            if let Some(dst) = color.get_mut(2 * x0..(2 * x1).min(color.len())) {
                composite_row(level, dst, px);
            }
        }
    }
}

#[cfg(test)]
#[path = "layer_tests.rs"]
mod layer_tests;
