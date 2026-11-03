//! What each particle draw covers and reads, for another renderer to draw the
//! same pixels: the numbers [`super::draw_particles`] works from, taken by the
//! same code, over a whole frame of `width` by `height`.

use super::{DecalDraw, SpriteDraw, StreakDraw, beam, decal, sprite};

/// A sprite's pixels and how each reads its mask.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpritePlan {
    /// Pixel columns and rows, each as `start..end`, on the screen.
    pub cols: (u32, u32),
    pub rows: (u32, u32),
    /// Top-left of the unclipped box, in pixels: a pixel's mask texel is
    /// `(centre - origin) * scale`, rounded down and held to the mip.
    pub origin: (f32, f32),
    pub scale: f32,
    /// Where the mip starts in [`super::SpriteBank::bytes`], and its side.
    pub start: u32,
    pub side: u32,
}

/// `None` for a sprite that draws nothing.
pub fn sprite_plan(s: &SpriteDraw, width: u32, height: u32) -> Option<SpritePlan> {
    let rows = (0, i32::try_from(height).ok()?);
    let l = sprite::layout(s, width, rows)?;
    let narrow = |v: usize| u32::try_from(v).ok();
    Some(SpritePlan {
        cols: (narrow(l.cols.0)?, narrow(l.cols.1)?),
        rows: (narrow(l.rows.0)?, narrow(l.rows.1)?),
        origin: l.origin,
        scale: l.scale,
        start: narrow(l.start)?,
        side: narrow(l.side)?,
    })
}

/// How a decal reads its mask: `1/z`, `s/z` and `t/z` as planes over the
/// screen, `value = base + gx * (x - origin.x) + gy * (y - origin.y)`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DecalPlan {
    pub origin: (f32, f32),
    pub base: [f32; 3],
    pub gx: [f32; 3],
    pub gy: [f32; 3],
    /// Where the mip starts in [`super::SpriteBank::bytes`], and its side.
    pub start: u32,
    pub side: u32,
    /// How hard the mask's centre darkens, `0..=256`.
    pub darkness: u32,
}

/// `None` for a decal that draws nothing. Its pixels are those of its points'
/// polygon whose centres it holds.
pub fn decal_plan(d: &DecalDraw) -> Option<DecalPlan> {
    let l = decal::layout(d)?;
    let narrow = |v: usize| u32::try_from(v).ok();
    Some(DecalPlan {
        origin: l.planes.origin,
        base: l.planes.base,
        gx: l.planes.gx,
        gy: l.planes.gy,
        start: narrow(l.start)?,
        side: narrow(l.side)?,
        darkness: l.darkness,
    })
}

/// Each pixel of `st` on a `width` by `height` screen: its column, row, `1/z`
/// and coverage. One beside the screen is left out.
pub fn streak_pixels(
    st: &StreakDraw,
    width: u32,
    height: u32,
    mut visit: impl FnMut(u32, u32, f32, u8),
) {
    let Ok(rows) = i32::try_from(height) else {
        return;
    };
    beam::walk(st, width, (0, rows), |x, y, inv_z, cover| {
        let (Ok(x), Ok(y)) = (u32::try_from(x), u32::try_from(y)) else {
            return;
        };
        if x < width && y < height {
            visit(x, y, inv_z, cover);
        }
    });
}
