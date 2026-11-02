//! The page of blocks: one custom-painted xui node. A click selects, a drag moves
//! a block, a drag of its right edge resizes it, the right button opens the menu,
//! and the wheel or a middle-button drag scrolls. A drag changes the recipe only
//! when it ends, so it is one undo step and costs no evaluation on its way.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use xui::app::Ui;
use xui::backend::{
    Canvas, Cursor, Event, NodeKind, NodeSpec, Result, TextStyle, TextVAlign, WidgetId,
};
use xui::geometry::{Rect, Size};
use xui::layout::Constraints;
use xui::units::Dip;
use xui::widget::{Control, Placeable};
use xui::{Image, MouseButton, Theme};

use super::TexMsg;
use super::page_geom::Grid;
use crate::model::texdoc::clamp_to_page;
use crate::texstudio::TexStudio;
use crate::ui::Msg;

/// How far the pointer must go before a press is a drag, in pixels.
const DRAG_START: i32 = 4;
/// Pixels the wheel scrolls per notch, at 96 DPI.
const WHEEL: i32 = 56;

/// What the pointer is doing.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
enum Drag {
    #[default]
    None,
    /// Pressed on a block, not yet moved.
    Press {
        block: usize,
        from: (i32, i32),
    },
    /// Moving a block to the cell given, which `grab` cells away from the pointer.
    Move {
        block: usize,
        grab: (i32, i32),
        cell: (u16, u16),
    },
    Resize {
        block: usize,
        w: u16,
    },
    Pan {
        last: (i32, i32),
    },
}

#[derive(Default)]
struct Shared {
    scroll: Cell<(i32, i32)>,
    drag: Cell<Drag>,
    /// By block index, as the worker last made them.
    thumbs: RefCell<Vec<Option<Image>>>,
}

/// The widget.
pub struct Page {
    control: Control<Msg>,
    shared: Rc<Shared>,
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

/// What a block looks like this frame.
struct Look<'a> {
    name: &'a str,
    selected: bool,
    broken: bool,
    thumb: Option<&'a Image>,
}

fn paint_block(canvas: &mut dyn Canvas, rect: Rect, look: &Look, theme: &Theme) {
    let inner = Rect::new(rect.left + 1, rect.top + 1, rect.right - 1, rect.bottom - 1);
    let (fill, ink, frame, width) = if look.broken {
        // Red, with a heavy frame: it reads as wrong without reading the text.
        let fill = theme.danger.lerp(theme.raised, 0.55);
        (fill, theme.text, theme.danger, 2.0)
    } else if look.selected {
        (theme.raised, theme.text, theme.accent, 2.0)
    } else {
        (theme.raised, theme.text, theme.border, 1.0)
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
    let style = TextStyle::new(ink, Dip(11.0)).middle();
    canvas.push_clip(text);
    canvas.draw_text(look.name, text, &style);
    canvas.pop_clip();
    canvas.stroke_rounded_rect(inner, 3.0, frame, width);
    if look.broken {
        let mut mark = TextStyle::new(theme.danger, Dip(14.0)).bold();
        mark.valign = TextVAlign::Middle;
        canvas.draw_text("!", thumb.shrink(side / 3), &mark);
    }
}

fn paint(canvas: &mut dyn Canvas, studio: &TexStudio, shared: &Shared, theme: &Theme) {
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
    for (i, block) in doc.recipe().blocks.iter().enumerate() {
        let rect = placed(grid.rect(block));
        if rect.left >= bounds.right || rect.top >= bounds.bottom {
            continue;
        }
        if rect.right <= bounds.left || rect.bottom <= bounds.top {
            continue;
        }
        let look = Look {
            name: block.spec.name,
            selected: doc.selected() == Some(i),
            broken: broken.contains(&i),
            thumb: thumbs.get(i).and_then(Option::as_ref),
        };
        paint_block(canvas, rect, &look, theme);
    }
    // Where the drag would put the block.
    let ghost = match shared.drag.get() {
        Drag::Move { block, cell, .. } => doc.recipe().blocks.get(block).map(|b| {
            let mut at = b.clone();
            (at.x, at.y) = cell;
            grid.rect(&at)
        }),
        Drag::Resize { block, w } => doc.recipe().blocks.get(block).map(|b| {
            let mut at = b.clone();
            at.w = w;
            grid.rect(&at)
        }),
        _ => None,
    };
    if let Some(rect) = ghost {
        canvas.stroke_rounded_rect(placed(rect), 3.0, theme.accent, 2.0);
    }
    canvas.stroke_rect(bounds, theme.border, 1.0);
}

/// One message from a mouse button going down.
fn press(c: &Ctx, x: i32, y: i32, button: MouseButton) -> Option<Msg> {
    let studio = c.studio.borrow();
    let blocks = &studio.doc()?.recipe().blocks;
    let grid = c.grid();
    let hit = grid.hit(blocks, x, y);
    match button {
        MouseButton::Left => {
            c.ui.focus(c.id);
            c.ui.set_capture(c.id);
            let block = hit.map(|h| h.block);
            if let Some(h) = hit {
                c.shared.drag.set(if h.on_right_edge {
                    let w = blocks.get(h.block).map_or(1, |b| b.w);
                    Drag::Resize { block: h.block, w }
                } else {
                    Drag::Press {
                        block: h.block,
                        from: (x, y),
                    }
                });
            }
            Some(Msg::Tex(TexMsg::Select(block)))
        }
        MouseButton::Middle => {
            c.ui.set_capture(c.id);
            c.shared.drag.set(Drag::Pan { last: (x, y) });
            None
        }
        MouseButton::Right => {
            let (cx, cy) = grid.cell_at(x, y);
            let cell = clamp_to_page(cx, cy, 1);
            Some(Msg::Tex(TexMsg::PageMenu {
                x,
                y,
                cell,
                block: hit.map(|h| h.block),
            }))
        }
        _ => None,
    }
}

/// Everything an event needs.
struct Ctx<'a> {
    ui: &'a Ui<Msg>,
    id: WidgetId,
    studio: &'a Rc<RefCell<TexStudio>>,
    shared: &'a Shared,
}

impl Ctx<'_> {
    fn grid(&self) -> Grid {
        Grid::new(self.ui.dpi(), self.shared.scroll.get())
    }

    fn scroll_by(&self, dx: i32, dy: i32) {
        let studio = self.studio.borrow();
        let blocks = studio
            .doc()
            .map_or(&[][..], |d| d.recipe().blocks.as_slice());
        let limit = self.grid().scroll_limit(blocks);
        let (sx, sy) = self.shared.scroll.get();
        self.shared.scroll.set((
            sx.saturating_add(dx).clamp(0, limit.0),
            sy.saturating_add(dy).clamp(0, limit.1),
        ));
        self.ui.invalidate(self.id);
    }

    /// The pointer moved during a drag.
    fn dragged(&self, x: i32, y: i32) {
        let studio = self.studio.borrow();
        let Some(doc) = studio.doc() else {
            return;
        };
        let grid = self.grid();
        let block_at = |b: usize| doc.recipe().blocks.get(b);
        let moved_to = |block: usize, grab: (i32, i32)| {
            let (cx, cy) = grid.cell_at(x, y);
            let w = block_at(block).map_or(1, |b| b.w);
            clamp_to_page(cx - grab.0, cy - grab.1, w)
        };
        match self.shared.drag.get() {
            Drag::Press { block, from } => {
                if (x - from.0).abs().max((y - from.1).abs()) < DRAG_START {
                    return;
                }
                let Some(b) = block_at(block) else {
                    return;
                };
                let (cx, cy) = grid.cell_at(from.0, from.1);
                let grab = (cx - i32::from(b.x), cy - i32::from(b.y));
                self.shared.drag.set(Drag::Move {
                    block,
                    grab,
                    cell: moved_to(block, grab),
                });
            }
            Drag::Move { block, grab, .. } => {
                self.shared.drag.set(Drag::Move {
                    block,
                    grab,
                    cell: moved_to(block, grab),
                });
            }
            Drag::Resize { block, .. } => {
                let Some(b) = block_at(block) else {
                    return;
                };
                let room = 64u16.saturating_sub(b.x).max(1);
                let w = grid.width_at(b, x).min(room);
                self.shared.drag.set(Drag::Resize { block, w });
            }
            Drag::Pan { last } => {
                drop(studio);
                self.shared.drag.set(Drag::Pan { last: (x, y) });
                self.scroll_by(last.0 - x, last.1 - y);
            }
            Drag::None => return,
        }
        self.ui.invalidate(self.id);
    }

    /// The cursor says what a press would do.
    fn hover(&self, x: i32, y: i32) {
        let studio = self.studio.borrow();
        let blocks = studio
            .doc()
            .map_or(&[][..], |d| d.recipe().blocks.as_slice());
        let on_edge = self
            .grid()
            .hit(blocks, x, y)
            .is_some_and(|h| h.on_right_edge);
        let cursor = if on_edge {
            Cursor::SizeHorizontal
        } else {
            Cursor::Default
        };
        self.ui.set_cursor(self.id, cursor);
    }

    fn release(&self) -> Option<Msg> {
        // In this order: letting go of the capture reports a capture change, which
        // would end the drag before it is read.
        let drag = self.shared.drag.replace(Drag::None);
        self.ui.release_capture();
        self.ui.invalidate(self.id);
        match drag {
            Drag::Move { block, cell, .. } => Some(Msg::Tex(TexMsg::Move {
                block,
                x: cell.0,
                y: cell.1,
            })),
            Drag::Resize { block, w } => Some(Msg::Tex(TexMsg::Resize { block, w })),
            _ => None,
        }
    }
}

fn handle(c: &Ctx, event: &Event) -> Option<Msg> {
    match event {
        Event::MouseDown { x, y, button, .. } => press(c, *x, *y, *button),
        Event::MouseMove { x, y, .. } => {
            if c.shared.drag.get() == Drag::None {
                c.hover(*x, *y);
            } else {
                c.dragged(*x, *y);
            }
            None
        }
        Event::MouseUp { .. } => c.release(),
        Event::CaptureChanged => {
            c.shared.drag.set(Drag::None);
            c.ui.invalidate(c.id);
            None
        }
        Event::MouseWheel {
            delta, horizontal, ..
        } => {
            let notch = (*delta as i32).signum() * WHEEL * c.ui.dpi() as i32 / 96;
            if *horizontal {
                c.scroll_by(notch, 0);
            } else {
                c.scroll_by(0, -notch);
            }
            None
        }
        _ => None,
    }
}

impl Page {
    pub fn new(ui: &Ui<Msg>, studio: Rc<RefCell<TexStudio>>) -> Result<Page> {
        let spec = NodeSpec::new(NodeKind::Custom, Rect::new(0, 0, 10, 10)).tab_stop();
        let control = Control::new(ui, &spec)?;
        let shared = Rc::new(Shared::default());
        {
            let (shared, studio) = (shared.clone(), studio.clone());
            let theme = ui.theme_handle();
            control.set_painter(Rc::new(move |canvas| {
                // Painting never fights an update for the model: it would miss
                // one frame, not take the editor down.
                if let Ok(studio) = studio.try_borrow() {
                    paint(canvas, &studio, &shared, &theme.get());
                }
            }));
        }
        {
            let shared = shared.clone();
            let ui = ui.clone();
            let id = control.id();
            control.on_events(move |event| {
                handle(
                    &Ctx {
                        ui: &ui,
                        id,
                        studio: &studio,
                        shared: &shared,
                    },
                    event,
                )
            });
        }
        Ok(Page { control, shared })
    }

    pub fn invalidate(&self) {
        self.control.invalidate();
    }

    pub fn focus(&self) {
        self.control.focus();
    }

    /// Shows new thumbnails.
    pub fn set_thumbs(&self, thumbs: Vec<Option<Image>>) {
        *self.shared.thumbs.borrow_mut() = thumbs;
        self.control.invalidate();
    }
}

impl Placeable<Msg> for Page {
    fn id(&self) -> WidgetId {
        self.control.id()
    }

    fn measure(&self, _ui: &Ui<Msg>, _constraints: Constraints) -> Size {
        Size::new(0, 0)
    }
}
