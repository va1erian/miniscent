//! The page of blocks: one custom-painted xui node. A click selects, a drag moves
//! a block, a drag of its right edge resizes it, the right button opens the menu,
//! and the wheel or a middle-button drag scrolls. A drag changes the recipe only
//! when it ends, so it is one undo step and costs no evaluation on its way. How it
//! is drawn is in `page_paint`.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use mscent_texgen::recipe::Block;
use xui::app::Ui;
use xui::backend::{Cursor, Event, NodeKind, NodeSpec, Result, WidgetId};
use xui::geometry::{Rect, Size};
use xui::layout::Constraints;
use xui::widget::{Control, Placeable};
use xui::{Image, MouseButton};

use super::TexMsg;
use super::page_geom::Grid;
use super::page_paint::paint;
use crate::model::texdoc::clamp_to_page;
use crate::texstudio::TexStudio;
use crate::ui::Msg;

/// How far the pointer must go before a press is a drag, in pixels.
const DRAG_START: i32 = 4;
/// Pixels the wheel scrolls per notch, at 96 DPI.
const WHEEL: i32 = 56;

/// What the pointer is doing.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub(super) enum Drag {
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
pub(super) struct Shared {
    pub(super) scroll: Cell<(i32, i32)>,
    pub(super) drag: Cell<Drag>,
    /// The material and its step of history when a block was taken: a drag holds a
    /// block index, which means another block, or none, once the page has changed.
    taken_from: RefCell<Option<(String, u64)>>,
    /// By block index, as the worker last made them.
    pub(super) thumbs: RefCell<Vec<Option<Image>>>,
}

/// The widget.
pub struct Page {
    control: Control<Msg>,
    shared: Rc<Shared>,
    ui: Ui<Msg>,
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
                *c.shared.taken_from.borrow_mut() = history_stamp(&studio);
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

/// Which page, at which step of its history, the pointer is working on.
fn history_stamp(studio: &TexStudio) -> Option<(String, u64)> {
    let doc = studio.doc()?;
    Some((doc.name().to_string(), doc.edits()))
}

/// Ends a drag with no effect; whether there was one.
fn end_drag(ui: &Ui<Msg>, id: WidgetId, shared: &Shared) -> bool {
    let was = shared.drag.replace(Drag::None) != Drag::None;
    if was {
        ui.release_capture();
        ui.invalidate(id);
    }
    was
}

/// Everything an event needs.
struct Ctx<'a> {
    ui: &'a Ui<Msg>,
    id: WidgetId,
    studio: &'a Rc<RefCell<TexStudio>>,
    shared: &'a Shared,
}

/// The node's size in pixels: how much of the page shows.
fn viewport(ui: &Ui<Msg>, id: WidgetId) -> (i32, i32) {
    let b = ui.bounds(id);
    (b.width(), b.height())
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
        let limit = self.grid().scroll_limit(blocks, viewport(self.ui, self.id));
        let (sx, sy) = self.shared.scroll.get();
        self.shared.scroll.set((
            sx.saturating_add(dx).clamp(0, limit.0),
            sy.saturating_add(dy).clamp(0, limit.1),
        ));
        self.ui.invalidate(self.id);
    }

    /// Whether the page changed since a block was taken: an undo, an added or
    /// deleted block, another material. A drag of a block is over then, with no
    /// move, or it would move whichever block now has that index.
    fn page_changed_under_drag(&self) -> bool {
        let holds_a_block = !matches!(self.shared.drag.get(), Drag::None | Drag::Pan { .. });
        holds_a_block && *self.shared.taken_from.borrow() != history_stamp(&self.studio.borrow())
    }

    /// The pointer moved during a drag.
    fn dragged(&self, x: i32, y: i32) {
        if self.page_changed_under_drag() {
            end_drag(self.ui, self.id, self.shared);
            return;
        }
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
        let stale = self.page_changed_under_drag();
        let drag = self.shared.drag.replace(Drag::None);
        self.ui.release_capture();
        self.ui.invalidate(self.id);
        if stale {
            return None;
        }
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
        Ok(Page {
            control,
            shared,
            ui: ui.clone(),
        })
    }

    pub fn invalidate(&self) {
        self.control.invalidate();
    }

    /// Ends a drag in progress with no effect, as Esc does; whether there was one.
    pub fn cancel_drag(&self) -> bool {
        end_drag(&self.ui, self.control.id(), &self.shared)
    }

    pub fn focus(&self) {
        self.control.focus();
    }

    /// Shows new thumbnails.
    pub fn set_thumbs(&self, thumbs: Vec<Option<Image>>) {
        *self.shared.thumbs.borrow_mut() = thumbs;
        self.control.invalidate();
    }

    pub fn reset_scroll(&self) {
        self.shared.scroll.set((0, 0));
        self.control.invalidate();
    }

    /// Scrolls the least that shows `block`, which is selected from somewhere
    /// other than the page, or is just where a stock recipe has it. False when the
    /// page has no size yet, as before its first layout: nothing was done.
    pub fn reveal(&self, block: &Block, blocks: &[Block]) -> bool {
        let grid = Grid::new(self.ui.dpi(), self.shared.scroll.get());
        let view = viewport(&self.ui, self.control.id());
        if view.0 <= 0 || view.1 <= 0 {
            return false;
        }
        let (x, y) = grid.revealing(block, view);
        let limit = grid.scroll_limit(blocks, view);
        self.shared
            .scroll
            .set((x.clamp(0, limit.0), y.clamp(0, limit.1)));
        self.control.invalidate();
        true
    }

    /// How far the page is scrolled, for scripted runs.
    #[cfg(test)]
    pub fn scroll(&self) -> (i32, i32) {
        self.shared.scroll.get()
    }

    /// Whether all of `block` is in view, for scripted runs.
    #[cfg(test)]
    pub fn shows(&self, block: &Block) -> bool {
        let view = viewport(&self.ui, self.control.id());
        let r = Grid::new(self.ui.dpi(), self.shared.scroll.get()).rect(block);
        r.left >= 0 && r.top >= 0 && r.right <= view.0 && r.bottom <= view.1
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
