//! 2D drawing into the colour buffer: rectangles, sprites and text.
//!
//! Everything clips to the framebuffer with 64-bit arithmetic, so any `i32`
//! position and size is safe, including ones far off screen. Colours are
//! `0xAARRGGBB`; alpha 255 overwrites, 0 skips, in between blends. The depth
//! buffer is left alone, since 2D is drawn last as an overlay.

use crate::font::{GLYPH_SIZE, glyph};
use crate::framebuffer::Framebuffer;

/// Intersection of a rectangle with the framebuffer as half-open pixel
/// ranges `(x0, y0, x1, y1)`, or `None` when nothing is visible.
fn clip_rect(
    fb: &Framebuffer,
    x: i32,
    y: i32,
    w: u32,
    h: u32,
) -> Option<(usize, usize, usize, usize)> {
    let x0 = i64::from(x).max(0);
    let y0 = i64::from(y).max(0);
    let x1 = (i64::from(x) + i64::from(w)).min(i64::from(fb.width()));
    let y1 = (i64::from(y) + i64::from(h)).min(i64::from(fb.height()));
    (x0 < x1 && y0 < y1).then_some((x0 as usize, y0 as usize, x1 as usize, y1 as usize))
}

/// Blend `src` over `dst` using the alpha in `src`'s top byte.
fn blend(dst: u32, src: u32) -> u32 {
    let a = src >> 24;
    match a {
        255 => src,
        0 => dst,
        _ => {
            let ch = |shift: u32| {
                let (s, d) = ((src >> shift) & 0xff, (dst >> shift) & 0xff);
                (s * a + d * (255 - a) + 127) / 255
            };
            0xff00_0000 | ch(16) << 16 | ch(8) << 8 | ch(0)
        }
    }
}

/// Fill a rectangle, clipped to the framebuffer.
pub fn fill_rect(fb: &mut Framebuffer, x: i32, y: i32, w: u32, h: u32, color: u32) {
    let Some((x0, y0, x1, y1)) = clip_rect(fb, x, y, w, h) else {
        return;
    };
    let stride = fb.width() as usize;
    let px = fb.pixels_mut();
    for row in y0..y1 {
        for p in &mut px[row * stride + x0..row * stride + x1] {
            *p = blend(*p, color);
        }
    }
}

/// Outline of a rectangle, `thickness` pixels wide, drawn inward. The four
/// bars never overlap, so translucent colours blend once; a rectangle too
/// small to have an interior is filled.
pub fn stroke_rect(
    fb: &mut Framebuffer,
    x: i32,
    y: i32,
    w: u32,
    h: u32,
    thickness: u32,
    color: u32,
) {
    let t = thickness;
    if t == 0 {
        return;
    }
    if t.saturating_mul(2) >= w.min(h) {
        fill_rect(fb, x, y, w, h, color);
        return;
    }
    let inner = h - 2 * t;
    let right = x.saturating_add_unsigned(w - t);
    let below = y.saturating_add_unsigned(h - t);
    let mid = y.saturating_add_unsigned(t);
    fill_rect(fb, x, y, w, t, color);
    fill_rect(fb, x, below, w, t, color);
    fill_rect(fb, x, mid, t, inner, color);
    fill_rect(fb, right, mid, t, inner, color);
}

/// An image with per-pixel alpha, for icons and the crosshair.
#[derive(Clone, Debug)]
pub struct Sprite {
    width: u32,
    height: u32,
    pixels: Vec<u32>,
}

impl Sprite {
    /// `None` unless `pixels.len() == width * height` and both are non-zero.
    pub fn new(width: u32, height: u32, pixels: Vec<u32>) -> Option<Self> {
        let n = (width as usize).checked_mul(height as usize)?;
        (n == pixels.len() && n > 0).then_some(Self {
            width,
            height,
            pixels,
        })
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }
}

/// Draw `sprite` with its top-left corner at `(x, y)`, clipped.
pub fn blit(fb: &mut Framebuffer, sprite: &Sprite, x: i32, y: i32) {
    let Some((x0, y0, x1, y1)) = clip_rect(fb, x, y, sprite.width, sprite.height) else {
        return;
    };
    let stride = fb.width() as usize;
    let sw = sprite.width as usize;
    // Offsets of the visible part inside the sprite; non-negative by clipping.
    let (ox, oy) = (
        (x0 as i64 - i64::from(x)) as usize,
        (y0 as i64 - i64::from(y)) as usize,
    );
    let px = fb.pixels_mut();
    for row in y0..y1 {
        let src = &sprite.pixels[(oy + row - y0) * sw + ox..][..x1 - x0];
        for (dst, &s) in px[row * stride + x0..row * stride + x1].iter_mut().zip(src) {
            *dst = blend(*dst, s);
        }
    }
}

/// Size in pixels of `text` at integer `scale` (newline starts a new line).
pub fn text_size(text: &str, scale: u32) -> (u32, u32) {
    let cell = GLYPH_SIZE as u32 * scale;
    let lines = text.split('\n');
    let height = lines.clone().count() as u32 * cell;
    let width = lines.map(|l| l.chars().count() as u32).max().unwrap_or(0) * cell;
    (width, height)
}

/// Draw `text` with its top-left at `(x, y)`; each glyph pixel becomes a
/// `scale` x `scale` block. A `scale` of 0 draws nothing.
pub fn draw_text(fb: &mut Framebuffer, x: i32, y: i32, text: &str, color: u32, scale: u32) {
    let cell = GLYPH_SIZE.saturating_mul(i32::try_from(scale).unwrap_or(i32::MAX));
    let (mut cx, mut cy) = (x, y);
    for ch in text.chars() {
        if ch == '\n' {
            cx = x;
            cy = cy.saturating_add(cell);
            continue;
        }
        draw_glyph(fb, cx, cy, ch, color, scale);
        cx = cx.saturating_add(cell);
    }
}

fn draw_glyph(fb: &mut Framebuffer, x: i32, y: i32, ch: char, color: u32, scale: u32) {
    let s = i32::try_from(scale).unwrap_or(i32::MAX);
    for (row, bits) in glyph(ch).iter().enumerate() {
        for col in 0..GLYPH_SIZE {
            if bits >> col & 1 == 1 {
                let px = x.saturating_add(col.saturating_mul(s));
                let py = y.saturating_add((row as i32).saturating_mul(s));
                fill_rect(fb, px, py, scale, scale, color);
            }
        }
    }
}

#[cfg(test)]
#[path = "draw2d_tests.rs"]
mod draw2d_tests;
