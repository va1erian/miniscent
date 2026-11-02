//! How the page looks: a grid, blocks with a thumbnail and their names, a mark on
//! the top edge of a block for each input it reads, and a bar where the page is
//! longer than the window shows.

use xui::backend::{Canvas, TextStyle, TextVAlign};
use xui::geometry::Rect;
use xui::units::Dip;
use xui::{Image, Theme};

use super::page::{Drag, Shared};
use super::page_geom::Grid;
use super::reads;
use crate::model::texdoc::TexDoc;
use crate::texstudio::TexStudio;

/// Width of the scroll bar at 96 DPI, and the least length of its thumb.
const BAR: i32 = 5;
const BAR_THUMB_MIN: i32 = 16;

/// What a block looks like this frame.
struct Look<'a> {
    name: &'a str,
    /// What a `store` or `load` is named, under its own name.
    detail: Option<&'a str>,
    selected: bool,
    broken: bool,
    thumb: Option<&'a Image>,
    /// Where it reads from: the window's x of a mark on its top edge for each input.
    marks: &'a [i32],
}

/// `text` cut short with an ellipsis until it is at most `width` pixels wide.
fn fitted(canvas: &dyn Canvas, text: &str, style: &TextStyle, width: i32) -> String {
    if canvas.measure_text(text, style).width <= width {
        return text.to_string();
    }
    let mut kept: Vec<char> = text.chars().collect();
    while kept.pop().is_some() {
        let candidate: String = kept.iter().chain(&['\u{2026}']).collect();
        if canvas.measure_text(&candidate, style).width <= width {
            return candidate;
        }
    }
    String::new()
}

fn draw_label(canvas: &mut dyn Canvas, look: &Look, area: Rect, theme: &Theme) {
    let name = TextStyle::new(theme.text, Dip(11.0)).middle();
    let Some(detail) = look.detail else {
        let text = fitted(canvas, look.name, &name, area.width());
        canvas.draw_text(&text, area, &name);
        return;
    };
    // Two lines: what the block is, then what it is named.
    let half = area.height() / 2;
    let (top, bottom) = (
        Rect::new(area.left, area.top, area.right, area.top + half),
        Rect::new(area.left, area.top + half, area.right, area.bottom),
    );
    let small = TextStyle::new(theme.text_secondary, Dip(10.0)).middle();
    let (first, second) = (
        fitted(canvas, look.name, &name, area.width()),
        fitted(canvas, detail, &small, area.width()),
    );
    canvas.draw_text(&first, top, &name);
    canvas.draw_text(&second, bottom, &small);
}

fn paint_block(canvas: &mut dyn Canvas, rect: Rect, look: &Look, grid: &Grid, theme: &Theme) {
    let inner = Rect::new(rect.left + 1, rect.top + 1, rect.right - 1, rect.bottom - 1);
    let (fill, frame, width) = if look.broken {
        // Red, with a heavy frame: it reads as wrong without reading the text.
        (theme.danger.lerp(theme.raised, 0.55), theme.danger, 2.0)
    } else if look.selected {
        (theme.raised, theme.accent, 2.0)
    } else {
        (theme.raised, theme.border, 1.0)
    };
    canvas.fill_rounded_rect(inner, 3.0, fill);
    let pad = 4;
    let side = inner.height() - pad * 2;
    let thumb = Rect::new(
        inner.left + pad,
        inner.top + pad,
        inner.left + pad + side,
        inner.top + pad + side,
    );
    if let Some(image) = look.thumb.filter(|_| !look.broken) {
        canvas.draw_image(image, thumb);
    } else {
        canvas.fill_rect(thumb, theme.input_background);
    }
    canvas.stroke_rect(thumb, theme.border, 1.0);
    let text = Rect::new(
        thumb.right + pad,
        inner.top,
        inner.right - pad,
        inner.bottom,
    );
    canvas.push_clip(text);
    draw_label(canvas, look, text, theme);
    canvas.pop_clip();
    canvas.stroke_rounded_rect(inner, 3.0, frame, width);
    // A tab on the top edge for each input, in the ink colour so that it stands
    // out from the frame, which is the accent when selected.
    let (half, depth) = (grid.edge / 2 + 1, grid.edge / 2 + 1);
    for &x in look.marks {
        let tab = Rect::new(x - half, rect.top + 1, x + half, rect.top + 1 + depth);
        canvas.fill_rect(tab, theme.text);
    }
    if look.broken {
        let mut mark = TextStyle::new(theme.danger, Dip(14.0)).bold();
        mark.valign = TextVAlign::Middle;
        canvas.draw_text("!", thumb.shrink(side / 3), &mark);
    }
}

fn paint_grid(canvas: &mut dyn Canvas, grid: &Grid, theme: &Theme) {
    let b = canvas.bounds();
    canvas.clear(theme.input_background);
    let line = theme.border.lerp(theme.input_background, 0.6);
    let mut x = b.left - grid.scroll.0.rem_euclid(grid.cell.0);
    while x < b.right {
        canvas.fill_rect(Rect::new(x, b.top, x + 1, b.bottom), line);
        x += grid.cell.0;
    }
    let mut y = b.top - grid.scroll.1.rem_euclid(grid.cell.1);
    while y < b.bottom {
        canvas.fill_rect(Rect::new(b.left, y, b.right, y + 1), line);
        y += grid.cell.1;
    }
}

/// Where the thumb of a bar `length` long is, as a start and an end: `shown` of
/// `whole` is in view, from `scroll`.
fn bar_span(scroll: i32, shown: i32, whole: i32, length: i32) -> (i32, i32) {
    let size = (length * shown / whole.max(1)).clamp(BAR_THUMB_MIN.min(length), length);
    let start = (length - size) * scroll / (whole - shown).max(1);
    (start, start + size)
}

/// A bar along one edge, where the page is longer than the window: its thumb
/// shows how much is in view and where.
fn paint_bars(canvas: &mut dyn Canvas, grid: &Grid, doc: &TexDoc, theme: &Theme) {
    let b = canvas.bounds();
    let content = grid.content(&doc.recipe().blocks);
    let width = (BAR * grid.edge / 8).max(2);
    let round = width as f32 / 2.0;
    if content.1 > b.height() {
        let (from, to) = bar_span(grid.scroll.1, b.height(), content.1, b.height());
        let rect = Rect::new(b.right - width - 1, b.top + from, b.right - 1, b.top + to);
        canvas.fill_rounded_rect(rect, round, theme.text_secondary);
    }
    if content.0 > b.width() {
        let (from, to) = bar_span(grid.scroll.0, b.width(), content.0, b.width());
        let rect = Rect::new(
            b.left + from,
            b.bottom - width - 1,
            b.left + to,
            b.bottom - 1,
        );
        canvas.fill_rounded_rect(rect, round, theme.text_secondary);
    }
}

pub(super) fn paint(canvas: &mut dyn Canvas, studio: &TexStudio, shared: &Shared, theme: &Theme) {
    let grid = Grid::new(canvas.dpi(), shared.scroll.get());
    paint_grid(canvas, &grid, theme);
    let Some(doc) = studio.doc() else {
        let style = TextStyle::new(theme.text_secondary, Dip(12.0)).middle();
        let mut area = canvas.bounds();
        area.left += 12;
        canvas.draw_text(
            "Pick a material on the left, or make a new one",
            area,
            &style,
        );
        return;
    };
    let bounds = canvas.bounds();
    // The grid is in the node's pixels; a canvas is in the window's.
    let placed = |r: Rect| {
        Rect::new(
            r.left + bounds.left,
            r.top + bounds.top,
            r.right + bounds.left,
            r.bottom + bounds.top,
        )
    };
    let (broken, _) = studio.trouble();
    let thumbs = shared.thumbs.borrow();
    let blocks = &doc.recipe().blocks;
    for (i, block) in blocks.iter().enumerate() {
        let rect = placed(grid.rect(block));
        if rect.left >= bounds.right || rect.top >= bounds.bottom {
            continue;
        }
        if rect.right <= bounds.left || rect.bottom <= bounds.top {
            continue;
        }
        // The compiled page says what is read; a page with an error says nothing.
        let sources = doc.page().as_ref().map_or(&[][..], |g| g.inputs_of(i));
        let marks: Vec<i32> = grid
            .mark_xs(blocks, i, sources)
            .into_iter()
            .map(|x| x + bounds.left)
            .collect();
        let look = Look {
            name: block.spec.name,
            detail: reads::slot_name(doc, i),
            selected: doc.selected() == Some(i),
            broken: broken.contains(&i),
            thumb: thumbs.get(i).and_then(Option::as_ref),
            marks: &marks,
        };
        paint_block(canvas, rect, &look, &grid, theme);
    }
    // Where the drag would put the block.
    let ghost = match shared.drag.get() {
        Drag::Move { block, cell, .. } => blocks.get(block).map(|b| {
            let mut at = b.clone();
            (at.x, at.y) = cell;
            grid.rect(&at)
        }),
        Drag::Resize { block, w } => blocks.get(block).map(|b| {
            let mut at = b.clone();
            at.w = w;
            grid.rect(&at)
        }),
        _ => None,
    };
    if let Some(rect) = ghost {
        canvas.stroke_rounded_rect(placed(rect), 3.0, theme.accent, 2.0);
    }
    paint_bars(canvas, &grid, doc, theme);
    canvas.stroke_rect(bounds, theme.border, 1.0);
}
