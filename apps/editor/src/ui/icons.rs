//! Vector icons for the tool palette. Icons xui already ships in its Lucide
//! set are used as they are; the few it lacks (a person, a light bulb, a flag)
//! are drawn here on the same 24-unit grid, with the same round-capped stroke,
//! so every button looks like one family. Colours come from the caller (the
//! theme), so the icons work in light and dark.

use xui::Color;
use xui::backend::{Canvas, Cap, Join, PathPlacement, PathSeg, Rgba, Stroke};
use xui::geometry::Rect;
use xui::icon::{Lucide, draw_icon};

use PathSeg::{Close, CubicTo as C, LineTo as L, MoveTo as M};

/// The design grid and stroke of the icon set.
const GRID: f32 = 24.0;
const STROKE: f32 = 1.75;

/// An icon on a tool button.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Glyph {
    /// One of xui's own Lucide icons.
    Lucide(Lucide),
    /// A standing figure: the player start.
    Person,
    /// A light bulb: a light entity.
    Bulb,
    /// A pennant on a pole: the goal.
    Flag,
}

// Head circle (centre 12,7 radius 3.5) and shoulders.
const PERSON: &[PathSeg] = &[
    M(15.5, 7.0),
    C(15.5, 8.93, 13.93, 10.5, 12.0, 10.5),
    C(10.07, 10.5, 8.5, 8.93, 8.5, 7.0),
    C(8.5, 5.07, 10.07, 3.5, 12.0, 3.5),
    C(13.93, 3.5, 15.5, 5.07, 15.5, 7.0),
    Close,
    M(5.0, 21.0),
    L(5.0, 19.5),
    C(5.0, 16.5, 8.0, 14.5, 12.0, 14.5),
    C(16.0, 14.5, 19.0, 16.5, 19.0, 19.5),
    L(19.0, 21.0),
    Close,
];

// Glass, neck and base.
const BULB: &[PathSeg] = &[
    M(12.0, 3.0),
    C(8.5, 3.0, 6.0, 5.6, 6.0, 8.8),
    C(6.0, 11.0, 7.2, 12.6, 8.5, 13.8),
    C(9.2, 14.5, 9.5, 15.2, 9.5, 16.0),
    L(9.5, 17.5),
    L(14.5, 17.5),
    L(14.5, 16.0),
    C(14.5, 15.2, 14.8, 14.5, 15.5, 13.8),
    C(16.8, 12.6, 18.0, 11.0, 18.0, 8.8),
    C(18.0, 5.6, 15.5, 3.0, 12.0, 3.0),
    Close,
    M(9.5, 20.0),
    L(14.5, 20.0),
    M(10.5, 22.0),
    L(13.5, 22.0),
    M(12.0, 17.5),
    L(12.0, 13.0),
];

// Pole and a pennant with a notch.
const FLAG: &[PathSeg] = &[
    M(5.0, 21.5),
    L(5.0, 3.0),
    M(5.0, 4.0),
    L(19.5, 4.0),
    L(15.5, 8.75),
    L(19.5, 13.5),
    L(5.0, 13.5),
    Close,
];

fn path(glyph: Glyph) -> &'static [PathSeg] {
    match glyph {
        Glyph::Person => PERSON,
        Glyph::Bulb => BULB,
        Glyph::Flag => FLAG,
        Glyph::Lucide(_) => &[],
    }
}

/// Draws `glyph` centred in `rect` (its shorter side is the icon size).
pub fn draw(canvas: &mut dyn Canvas, glyph: Glyph, rect: Rect, color: Color, dpi: u32) {
    if let Glyph::Lucide(icon) = glyph {
        draw_icon(canvas, icon, rect, color, dpi);
        return;
    }
    let size = rect.width().min(rect.height());
    if size < 3 {
        return;
    }
    let scale = size as f32 / GRID;
    let at = PathPlacement::new(
        scale,
        (rect.left + rect.right - size) as f32 / 2.0,
        (rect.top + rect.bottom - size) as f32 / 2.0,
    );
    let width = (STROKE * scale).max(1.25 * dpi as f32 / 96.0);
    let stroke = Stroke::new(width).cap(Cap::Round).join(Join::Round);
    canvas.stroke_path(path(glyph), at, Rgba::from(color), &stroke);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn custom_icons_stay_on_the_grid() {
        for glyph in [Glyph::Person, Glyph::Bulb, Glyph::Flag] {
            let segs = path(glyph);
            assert!(!segs.is_empty());
            for seg in segs {
                let coords: &[f32] = match seg {
                    PathSeg::MoveTo(x, y) | PathSeg::LineTo(x, y) => &[*x, *y],
                    PathSeg::CubicTo(a, b, c, d, e, f) => &[*a, *b, *c, *d, *e, *f],
                    PathSeg::Close => &[],
                };
                assert!(coords.iter().all(|v| (0.0..=GRID).contains(v)), "{glyph:?}");
            }
        }
    }
}
