//! The workspace on real widgets through xui's offscreen backend: real events into
//! the block page and the keyboard, with the recipe read back.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::{Duration, Instant};

use xui::backend::Event;
use xui::message::{Modifiers, MouseButton};
use xui::widget::Placeable;
use xui::xui_canvas::snapshot::{Snapshot, Stage, render_with};
use xui::{Dip, Key, Theme};

use super::dialogs_tests::NewFormProbe;
use crate::model::Document;
use crate::studio::tick_costs::TickCosts;
use crate::texstudio::{Tab, TexStudio};
use crate::ui::texstudio::{Page, TexMsg};
use crate::ui::{Editor, Msg, cmd};

/// What a script reaches in the built editor.
pub(super) struct Handles {
    /// The widgets of the New form.
    pub(super) new_form: Rc<NewFormProbe>,
    pub(super) state: Rc<RefCell<crate::model::EditorState>>,
    pub(super) studio: Rc<RefCell<TexStudio>>,
    pub(super) page: Rc<Page>,
    pub(super) menu: Rc<xui::widget::Menu<Msg>>,
    pub(super) preview: Rc<super::TexPreview>,
    /// Where the page is in the window.
    pub(super) origin: Box<dyn Fn() -> (i32, i32)>,
}

/// Builds the editor and runs `script` with the workspace open on a new material.
pub(super) fn with_workspace(script: impl FnOnce(&Stage<'_, Msg>, &Handles) + 'static) {
    let handles: Rc<RefCell<Option<Handles>>> = Rc::new(RefCell::new(None));
    let ran = Rc::new(Cell::new(false));
    let (h2, r2) = (handles.clone(), ran.clone());
    render_with(
        Snapshot::new(Dip(1280.0), Dip(800.0)).theme(Theme::light()),
        move |ui| {
            let app = Editor::build(ui, Document::from_level(&mscent_world::showcase_level()))?;
            let page = app.tex.page.get();
            let (host, page_id, root) = (app.tex.host.get(), Placeable::id(&*page), ui.clone());
            let origin = Box::new(move || {
                let (outer, inner) = (root.bounds(host.id()), root.bounds(page_id));
                (outer.left + inner.left, outer.top + inner.top)
            });
            let new_form = Rc::new(NewFormProbe::of(ui, &app.tex));
            *h2.borrow_mut() = Some(Handles {
                new_form,
                state: app.state.clone(),
                studio: app.tex.studio.clone(),
                page,
                menu: app.menu.get(),
                preview: app.tex.preview.get(),
                origin,
            });
            Ok(app)
        },
        move |stage| {
            let handles = handles.borrow();
            let handles = handles.as_ref().expect("built");
            stage.emit(Msg::Menu(cmd::TEX_OPEN));
            stage.emit(Msg::Tex(TexMsg::Create {
                name: "ui-test".to_string(),
                side: 16,
            }));
            // The first material is being read on the worker.
            wait_opened(stage, handles);
            let _ = stage.ui().capture();
            script(stage, handles);
            r2.set(true);
        },
    )
    .expect("render");
    assert!(ran.get(), "the script ran to the end");
}

/// Ticks until the worker has read the recipe being opened, if one is.
fn wait_opened(stage: &Stage<'_, Msg>, h: &Handles) {
    let end = Instant::now() + Duration::from_secs(60);
    while h.studio.borrow().is_opening() {
        stage.emit(Msg::Step);
        assert!(Instant::now() < end, "never opened");
        std::thread::sleep(Duration::from_millis(1));
    }
}

/// Opens a material of the table and waits for its recipe.
pub(super) fn open(stage: &Stage<'_, Msg>, h: &Handles, name: &str) {
    stage.emit(Msg::Tex(TexMsg::Open(name.to_string())));
    wait_opened(stage, h);
}

pub(super) fn mouse_event(kind: u8, x: i32, y: i32, button: MouseButton) -> Event {
    let modifiers = Modifiers::NONE;
    match kind {
        0 => Event::MouseDown {
            x,
            y,
            button,
            modifiers,
        },
        1 => Event::MouseMove { x, y, modifiers },
        _ => Event::MouseUp {
            x,
            y,
            button,
            modifiers,
        },
    }
}

/// Injects a mouse event at a point of the page.
pub(super) fn at_page(stage: &Stage<'_, Msg>, h: &Handles, kind: u8, x: i32, y: i32) {
    let (ox, oy) = (h.origin)();
    stage.inject(mouse_event(kind, ox + x, oy + y, MouseButton::Left));
}

pub(super) fn press(stage: &Stage<'_, Msg>, key: Key, modifiers: Modifiers) {
    stage.inject(Event::KeyDown {
        key,
        modifiers,
        repeat: 1,
        system: false,
    });
    stage.inject(Event::KeyUp {
        key,
        modifiers,
        system: false,
    });
}

pub(super) const CTRL: Modifiers = Modifiers {
    ctrl: true,
    ..Modifiers::NONE
};

/// The middle of a cell of the page, in the page's pixels at 96 DPI.
pub(super) fn cell(x: i32, y: i32) -> (i32, i32) {
    (x * 144 + 50, y * 56 + 28)
}

pub(super) fn blocks(h: &Handles) -> Vec<(u16, u16, u16)> {
    let studio = h.studio.borrow();
    let doc = studio.doc().expect("open");
    let list = doc.recipe().blocks.iter();
    list.map(|b| (b.x, b.y, b.w)).collect()
}

#[test]
fn a_click_selects_a_drag_moves_and_the_right_edge_resizes() {
    with_workspace(|stage, h| {
        assert_eq!(blocks(h), [(0, 0, 1), (0, 1, 1)], "the starter page");
        // A click on the second block.
        let (x, y) = cell(0, 1);
        at_page(stage, h, 0, x, y);
        at_page(stage, h, 2, x, y);
        assert_eq!(h.studio.borrow().doc().expect("open").selected(), Some(1));
        assert_eq!(blocks(h), [(0, 0, 1), (0, 1, 1)], "a click moves nothing");
        // A click on nothing clears it.
        let (x, y) = cell(5, 2);
        at_page(stage, h, 0, x, y);
        at_page(stage, h, 2, x, y);
        assert_eq!(h.studio.borrow().doc().expect("open").selected(), None);
        // A drag: nothing changes until it ends, and then all at once.
        let (x, y) = cell(0, 1);
        let (tx, ty) = cell(2, 3);
        at_page(stage, h, 0, x, y);
        at_page(stage, h, 1, tx, ty);
        assert_eq!(blocks(h)[1], (0, 1, 1), "not until it ends");
        at_page(stage, h, 2, tx, ty);
        assert_eq!(blocks(h)[1], (2, 3, 1));
        // The right edge: one column more.
        let right = 3 * 144 - 3;
        let top = 3 * 56 + 20;
        at_page(stage, h, 0, right, top);
        at_page(stage, h, 1, right + 144, top);
        at_page(stage, h, 2, right + 144, top);
        assert_eq!(blocks(h)[1], (2, 3, 2));
        // Two edits, two undo steps, by the keyboard, leaving the level alone.
        let level = h.state.borrow().doc.clone();
        press(stage, Key::Z, CTRL);
        assert_eq!(blocks(h)[1], (2, 3, 1));
        press(stage, Key::Z, CTRL);
        assert_eq!(blocks(h)[1], (0, 1, 1));
        press(stage, Key::Y, CTRL);
        assert_eq!(blocks(h)[1], (2, 3, 1));
        assert!(h.state.borrow().doc == level, "undo went to the recipe");
    });
}

#[test]
fn level_shortcuts_stay_away_from_the_level_while_the_workspace_is_up() {
    with_workspace(|stage, h| {
        let (level, tool) = {
            let st = h.state.borrow();
            (st.doc.clone(), st.tool)
        };
        h.studio.borrow_mut().edit(|d| {
            d.select(Some(0));
            Ok(())
        });
        // Tool letters, the slice keys, Play, New and Open do nothing.
        for key in [Key::B, Key::E, Key::C, Key::V, Key::K, Key::RETURN, Key::F5] {
            press(stage, key, Modifiers::NONE);
        }
        for key in [Key::R, Key::N, Key::E] {
            press(stage, key, CTRL);
        }
        {
            let st = h.state.borrow();
            assert_eq!(st.tool, tool);
            assert!(st.doc == level);
            assert!(!st.message.starts_with("Playing"), "{}", st.message);
        }
        // Delete goes to the block.
        press(stage, Key::DELETE, Modifiers::NONE);
        assert_eq!(blocks(h).len(), 1, "the selected block went");
        assert!(h.state.borrow().doc == level);
        press(stage, Key::Z, CTRL);
        assert_eq!(blocks(h).len(), 2);
        assert!(h.studio.borrow().is_open(), "still up");
        // Escape goes back, and now the shortcuts are the level's again.
        assert!(!h.menu.is_enabled(cmd::PLAY), "the menu says so too");
        press(stage, Key::ESCAPE, Modifiers::NONE);
        assert!(!h.studio.borrow().is_open());
        assert!(h.menu.is_enabled(cmd::PLAY) && h.menu.is_enabled(cmd::NEW));
        press(stage, Key::B, Modifiers::NONE);
        assert_ne!(h.state.borrow().tool, tool, "tool letters work again");
    });
}

/// The bounding box of what differs between two pictures of one size.
fn changed_box(a: &xui::Image, b: &xui::Image) -> Option<(u32, u32, u32, u32)> {
    let (w, h) = (a.width(), a.height());
    let mut found: Option<(u32, u32, u32, u32)> = None;
    for y in 0..h {
        for x in 0..w {
            if a.pixel(x, y) != b.pixel(x, y) {
                let g = found.get_or_insert((x, y, x, y));
                *g = (g.0.min(x), g.1.min(y), g.2.max(x), g.3.max(y));
            }
        }
    }
    found
}

/// A picture once the window has stopped changing. The parameter panel is rebuilt
/// for a new selection and lays out a frame after the first picture that shows it,
/// so a "before" taken at once would count that as part of what a popup changed.
pub(super) fn steady_shot(stage: &Stage<'_, Msg>) -> xui::Image {
    let mut last = stage.ui().capture().expect("captures");
    for _ in 0..8 {
        let next = stage.ui().capture().expect("captures");
        if next == last {
            break;
        }
        last = next;
    }
    last
}

/// Real events through the real popups: the right button, the operator's group and
/// the operator. Where things are is read from the pictures, so a longer name in the
/// table does not break this.
#[test]
fn the_right_button_adds_an_operator_and_deletes_a_block() {
    with_workspace(|stage, h| {
        let (ox, oy) = (h.origin)();
        let shot = || steady_shot(stage);
        let right = |x: i32, y: i32, kind| {
            stage.inject(mouse_event(kind, ox + x, oy + y, MouseButton::Right));
        };
        let left = |x: i32, y: i32| {
            stage.inject(mouse_event(0, x, y, MouseButton::Left));
            stage.inject(mouse_event(2, x, y, MouseButton::Left));
        };
        let before = blocks(h).len();
        // On an empty cell: the menu opens at the pointer.
        let (x, y) = cell(3, 0);
        let closed = shot();
        right(x, y, 0);
        right(x, y, 2);
        let open = shot();
        let popup = changed_box(&closed, &open).expect("a menu showed");
        assert!(
            (popup.0 as i32 - (ox + x)).abs() < 8 && (popup.1 as i32 - (oy + y)).abs() < 8,
            "the menu is at the pointer: {popup:?}, pointer {}, {}",
            ox + x,
            oy + y
        );
        // Its first row is the first group, whose operators open beside it.
        let row = (popup.0 as i32 + 30, popup.1 as i32 + 14);
        left(row.0, row.1);
        let with_group = shot();
        let group = changed_box(&open, &with_group).expect("a submenu showed");
        left(group.2 as i32 - 12, row.1);
        assert_eq!(blocks(h).len(), before + 1, "the first operator was added");
        let added = *blocks(h).last().expect("a block");
        assert_eq!((added.0, added.1), (3, 0), "at the cell under the pointer");
        // On that block, which the adding selected: Delete is the last row.
        let closed = shot();
        right(x, y, 0);
        right(x, y, 2);
        let open = shot();
        let popup = changed_box(&closed, &open).expect("a menu showed");
        let last_row = (popup.0 as i32 + 30, popup.3 as i32 - 16);
        left(last_row.0, last_row.1);
        assert_eq!(blocks(h).len(), before, "deleted");
    });
}

/// The menu is grouped, so it is short, and it stays inside the window where the
/// pointer is at the bottom of the page.
#[test]
fn the_add_menu_is_grouped_and_fits_the_window_at_the_page_bottom() {
    with_workspace(|stage, h| {
        let (ox, oy) = (h.origin)();
        let closed = steady_shot(stage);
        let (x, y) = cell(3, 6);
        for kind in [0, 2] {
            stage.inject(mouse_event(kind, ox + x, oy + y, MouseButton::Right));
        }
        let popup = changed_box(&closed, &steady_shot(stage)).expect("a menu showed");
        let mut groups: Vec<&str> = Vec::new();
        for op in mscent_texgen::ops::all() {
            if !groups.contains(&op.group) {
                groups.push(op.group);
            }
        }
        // A row is under 30 pixels: one per group, a rule and two commands.
        let rows = groups.len() as u32 + 3;
        assert!(popup.3 - popup.1 < 30 * rows, "short: {popup:?}");
        assert!(popup.2 < 1280 && popup.3 < 800, "inside: {popup:?}");
    });
}

#[test]
fn the_workspace_lays_out_without_warnings() {
    with_workspace(|stage, h| {
        h.studio.borrow_mut().set_tab(Tab::Albedo);
        let report = stage.ui().layout_report();
        let warnings: Vec<&str> = report
            .lines()
            .filter(|l| l.trim_start().starts_with("! "))
            .collect();
        assert!(warnings.is_empty(), "{warnings:#?}");
    });
}

#[test]
fn a_slider_dragged_through_messages_is_one_undo_step() {
    with_workspace(|stage, h| {
        let index_of_float = || {
            let studio = h.studio.borrow();
            let doc = studio.doc().expect("open");
            let params = doc.recipe().blocks[0].spec.params;
            params
                .iter()
                .position(|p| matches!(p.kind, mscent_texgen::ops::ParamKind::Color { .. }))
        };
        let index = index_of_float().expect("the first generator of the table has a colour");
        let colour = |v: f32| mscent_texgen::ops::Value::Color([v, v, v, 1.0]);
        let param = |v, live| {
            Msg::Tex(TexMsg::Param {
                block: 0,
                index,
                value: colour(v),
                live,
            })
        };
        let before = h.studio.borrow().doc().expect("open").recipe().clone();
        for v in [0.1, 0.2, 0.3] {
            stage.emit(param(v, true));
        }
        stage.emit(param(0.4, false));
        press(stage, Key::Z, CTRL);
        let now = h.studio.borrow().doc().expect("open").recipe().clone();
        assert!(now == before, "one step back is before the drag");
    });
}

#[test]
fn the_page_paints_broken_blocks_differently_and_never_panics_on_a_hostile_page() {
    with_workspace(|stage, h| {
        let shot = || stage.ui().capture().expect("captures");
        let page = h.page.clone();
        // Where the second block is drawn, in the window.
        let (ox, oy) = (h.origin)();
        let (x, y) = cell(0, 1);
        let at = (ox + x + 30, oy + y - 20);
        let good = shot().pixel(at.0 as u32, at.1 as u32).expect("pixel");
        // Off its input: no block above it any more, so it is broken.
        stage.emit(Msg::Tex(TexMsg::Move {
            block: 0,
            x: 5,
            y: 0,
        }));
        page.invalidate();
        let bad = shot().pixel(at.0 as u32, at.1 as u32).expect("pixel");
        assert_ne!(good, bad, "a broken block must look different");
        assert!(bad[0] > bad[2], "reddish: {bad:?}");
        // Off the page entirely, and paint goes on.
        stage.emit(Msg::Tex(TexMsg::Move {
            block: 1,
            x: u16::MAX,
            y: u16::MAX,
        }));
        stage.emit(Msg::Tex(TexMsg::Resize {
            block: 1,
            w: u16::MAX,
        }));
        page.invalidate();
        shot();
        assert!(h.studio.borrow().trouble().1.is_some());
    });
}

/// Real widgets and the real tick. That ticks never wait is proved without a clock
/// in `texstudio/tick_tests.rs`; this is what they cost as a rule.
#[test]
fn a_drag_on_real_widgets_keeps_ticks_cheap() {
    with_workspace(|stage, h| {
        stage.emit(Msg::Tex(TexMsg::Create {
            name: "large".to_string(),
            side: 1024,
        }));
        let index = {
            let studio = h.studio.borrow();
            let params = studio.doc().expect("open").recipe().blocks[0].spec.params;
            let colour = |p: &mscent_texgen::ops::ParamSpec| {
                matches!(p.kind, mscent_texgen::ops::ParamKind::Color { .. })
            };
            params.iter().position(colour).expect("a colour")
        };
        let mut costs = TickCosts::default();
        for i in 0..300 {
            let v = (i % 100) as f32 / 100.0;
            costs.timed(|| {
                stage.emit(Msg::Tex(TexMsg::Param {
                    block: 0,
                    index,
                    value: mscent_texgen::ops::Value::Color([v, v, v, 1.0]),
                    live: true,
                }));
                stage.emit(Msg::Step);
            });
            std::thread::sleep(Duration::from_millis(1));
        }
        costs.assert_cheap();
    });
}

#[test]
fn a_picture_reaches_the_preview_with_nothing_but_ticks() {
    with_workspace(|stage, h| {
        h.studio.borrow_mut().set_tab(Tab::Albedo);
        let end = Instant::now() + Duration::from_secs(60);
        while !h.studio.borrow().settled() {
            stage.emit(Msg::Step);
            assert!(Instant::now() < end, "never settled");
            std::thread::sleep(Duration::from_millis(1));
        }
    });
}
