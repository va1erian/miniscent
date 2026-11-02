//! The `text` operator: a line of the bitmap font, drawn hard-edged.

use super::font::{GLYPH_H, GLYPH_W, glyph};
use super::util::{BLACK, WHITE, canvas_at, over};
use super::{Ctx, Inputs, MAX_TEXT, OpSpec, Role, color, float, text};
use crate::error::TexError;
use crate::image::{Image, to_px};
use crate::math::wrap_delta;

/// Columns a character takes: its glyph and one of space.
const ADVANCE: usize = GLYPH_W + 1;

/// Whether the font dot at `(col, row)` of the line is inked.
fn inked(glyphs: &[[u8; GLYPH_H]], col: usize, row: usize) -> bool {
    let within = col % ADVANCE;
    if within >= GLYPH_W || row >= GLYPH_H {
        return false;
    }
    glyphs
        .get(col / ADVANCE)
        .is_some_and(|g| (g[row] >> (GLYPH_W - 1 - within)) & 1 == 1)
}

/// Distance from `origin` going right or down, folded into one tile: text that
/// runs off one edge continues from the other.
fn along(u: f32, origin: f32) -> f32 {
    let d = wrap_delta(u - origin);
    if d < 0.0 { d + 1.0 } else { d }
}

fn text_op(ctx: &Ctx) -> Result<Image, TexError> {
    let glyphs: Vec<[u8; GLYPH_H]> = ctx.text("text").chars().take(MAX_TEXT).map(glyph).collect();
    let (x, y, scale) = (ctx.float("x"), ctx.float("y"), ctx.float("scale"));
    let size = ctx.frame.size;
    // A font dot is `scale` of the width wide and square in texels.
    let (dot_u, dot_v) = (scale, scale * size.width() as f32 / size.height() as f32);
    let color = ctx.color("color");
    Ok(Image::from_xy(size, |px, py| {
        let (u, v) = ctx.frame.uv(px, py);
        let (col, row) = (along(u, x) / dot_u, along(v, y) / dot_v);
        let on = dot_u > 0.0 && inked(&glyphs, col as usize, row as usize);
        to_px(over(canvas_at(ctx, px, py), color, f32::from(u8::from(on))))
    }))
}

pub(super) const OPS: &[OpSpec] = &[OpSpec {
    name: "text",
    group: "Draw",
    inputs: Inputs { min: 0, max: 1 },
    params: &[
        text("text", "TEXT"),
        float("x", 0.0, 1.0, 0.1),
        float("y", 0.0, 1.0, 0.4),
        float("scale", 0.0, 0.25, 0.02),
        color("color", WHITE),
        color("background", BLACK),
    ],
    role: Role::Kernel,
    kernel: text_op,
}];
