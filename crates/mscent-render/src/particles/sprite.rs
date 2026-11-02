//! Sprite drawing: clip the box to the band, pick a mip, build the column
//! table, then for each row gather the mask bytes and hand the row to a kernel.
//!
//! A mask pixel is chosen by nearest scale from the position relative to the
//! sprite's unclipped box, so a pixel's value does not depend on how the box
//! was clipped (screen edge or band edge) and bands agree with a whole frame.
//!
//! The column table and the gathered mask row live on the stack. A row can be
//! at most [`TABLE`] pixels wide because the radius is clamped to
//! [`MAX_RADIUS`]; a sprite asking for more is drawn at that radius instead.

use super::bank::{MIPS, SpriteBank};
use super::kernels::blend_row;
use super::{Blend, SpriteDraw};
use crate::framebuffer::Framebuffer;
use crate::simd::Level;

/// Largest drawn radius in pixels, which bounds a row at [`TABLE`] pixels.
pub(super) const MAX_RADIUS: f32 = 512.0;
const TABLE: usize = 1024;
/// Largest mask side, and so the length of the copied mask row.
const MAX_SIDE: usize = 128;
/// Pixels gathered and depth-checked at a time.
const BLOCK: usize = 64;

/// A sprite clipped to the band.
struct Plan<'a> {
    /// Columns to draw, as a pixel range.
    cols: (usize, usize),
    /// Rows to draw, in screen coordinates.
    rows: (usize, usize),
    /// Top-left of the unclipped box, in pixels.
    origin: (f32, f32),
    /// Mask pixels per screen pixel.
    scale: f32,
    /// The chosen mip, row-major, `side` pixels square.
    mask: &'a [u8],
    side: usize,
}

pub(super) fn draw_sprite(
    level: Level,
    fb: &mut Framebuffer,
    s: &SpriteDraw,
    blend: Blend,
    bank: &SpriteBank,
) {
    // `inv_z > 0` also rejects NaN and anything at or behind the eye.
    let valid = s.x.is_finite() && s.y.is_finite() && s.radius.is_finite();
    if !valid || s.radius <= 0.0 || !s.inv_z.is_finite() || s.inv_z <= 0.0 {
        return;
    }
    let radius = s.radius.min(MAX_RADIUS);
    let (left, top) = (s.x - radius, s.y - radius);
    let diameter = 2.0 * radius;
    let (rows0, rows1) = fb.row_range();
    let (x0, x1) = span(left, diameter, 0.0, fb.width() as f32);
    let (y0, y1) = span(top, diameter, rows0 as f32, rows1 as f32);
    if x1 <= x0 || y1 <= y0 {
        return;
    }
    let (mask, side) = bank.mip(s.mask, mip_for(diameter));
    let scale = side as f32 / diameter;
    if !scale.is_finite() || mask.len() != side * side {
        return;
    }
    let plan = Plan {
        cols: (x0, x1.min(x0 + TABLE)),
        rows: (y0, y1),
        origin: (left, top),
        scale,
        mask,
        side,
    };
    // Stack scratch sized to the sprite: zeroing a full table would cost more
    // than drawing a small sprite.
    match plan.cols.1 - plan.cols.0 {
        0..=32 => fill::<32>(level, fb, &plan, s, blend),
        33..=128 => fill::<128>(level, fb, &plan, s, blend),
        _ => fill::<TABLE>(level, fb, &plan, s, blend),
    }
}

/// Draw a plan whose columns fit in `N` pixels.
fn fill<const N: usize>(
    level: Level,
    fb: &mut Framebuffer,
    plan: &Plan,
    s: &SpriteDraw,
    blend: Blend,
) {
    let (x0, x1) = plan.cols;
    let (n, side) = (x1 - x0, plan.side);
    let mut columns = [0u8; N];
    for (i, c) in columns.iter_mut().enumerate().take(n) {
        *c = nearest((x0 + i) as f32 + 0.5 - plan.origin.0, plan.scale, side);
    }
    let mut gathered = [0u8; N];
    // The mask row is copied to a fixed array so indexing it by a column
    // (always below `side <= 128`) needs no bounds check.
    let mut row = [0u8; MAX_SIDE];
    for y in plan.rows.0..plan.rows.1 {
        let v = usize::from(nearest(y as f32 + 0.5 - plan.origin.1, plan.scale, side));
        let Some(src) = plan.mask.get(v * side..(v + 1) * side) else {
            return;
        };
        row[..side].copy_from_slice(src);
        let Some((color, depth)) = fb.row_mut(y as i32) else {
            continue;
        };
        for start in (0..n).step_by(BLOCK) {
            let end = (start + BLOCK).min(n);
            let depth = &depth[x0 + start..x0 + end];
            // Gathering costs more than the blend, so a block hidden behind
            // the scene is skipped before it.
            if !depth.iter().fold(false, |any, &z| any | (s.inv_z >= z)) {
                continue;
            }
            for (g, &c) in gathered[start..end].iter_mut().zip(&columns[start..end]) {
                *g = row[usize::from(c) % MAX_SIDE];
            }
            blend_row(
                level,
                blend,
                &mut color[x0 + start..x0 + end],
                depth,
                &gathered[start..end],
                s.tint,
                s.inv_z,
            );
        }
    }
}

/// Pixels `a..b` of `lo..hi` whose centres lie in `[start, start + len)`.
/// Compared in `f32` before converting, so any finite or huge value clips
/// instead of overflowing.
fn span(start: f32, len: f32, lo: f32, hi: f32) -> (usize, usize) {
    let edge = |v: f32| (v - 0.5).ceil().clamp(lo, hi);
    (edge(start) as usize, edge(start + len) as usize)
}

/// Index of the mask pixel under a position `pos` pixels into the box.
fn nearest(pos: f32, scale: f32, side: usize) -> u8 {
    // Float to integer casts saturate and map NaN to zero, so rounding
    // error at the far edge lands on the last pixel rather than outside.
    ((pos * scale) as usize).min(side - 1) as u8
}

/// The smallest mip at least as large as the sprite, so scaling is a
/// downsample (the next mip up).
fn mip_for(diameter: f32) -> usize {
    (0..MIPS - 1)
        .find(|&level| (8usize << level) as f32 >= diameter)
        .unwrap_or(MIPS - 1)
}

#[cfg(test)]
#[path = "sprite_tests.rs"]
mod sprite_tests;
