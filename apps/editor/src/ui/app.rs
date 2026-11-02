//! The editor application: menus, tool palette, viewports and the status
//! bar, wired to the shared [`EditorState`]. File and Play commands live in
//! `commands.rs`. The 3D pane follows the document by itself: see
//! `studio/preview.rs`.

use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;

use mscent_world::Op;
use xui::prelude::*;
use xui::widget::{FileDialog, HasText};

use super::commands::menus;
use super::keys::{OPS, TOOLS, key_name, shortcut};
use super::layouts::{self, Arrangement, Disposition};
pub use super::messages::{Msg, cmd};
use super::props::{Panel, apply};
use super::texstudio::Workspace;
use super::toolbar::IconButton;
use super::view3d::View3d;
use super::viewport::ViewEvent;
use super::wrap::{LOG_COLS, LOG_ROWS, wrap};
use crate::files;
use crate::model::{Document, EditorState, state::GRIDS};
use crate::studio::{HELP, Mode, Preview, Studio};

/// Width of the tool palette on the left, in dip.
const PALETTE_WIDTH: i32 = 130;

/// Width of the right-hand column (properties and log), in dip.
const SIDE_WIDTH: i32 = 340;

/// Height of the log box, in dip.
const LOG_HEIGHT: i32 = 110;

pub struct Editor {
    pub(super) state: Rc<RefCell<EditorState>>,
    pub(super) classic: Arrangement,
    pub(super) quad: Arrangement,
    pub(super) disposition: Disposition,
    pub(super) play: Handle<View3d<Msg>>,
    pub(super) menu: Handle<Menu<Msg>>,
    pub(super) status: Handle<StatusBar<Msg>>,
    tools: [Handle<IconButton<Msg>>; TOOLS.len()],
    ops: [Handle<IconButton<Msg>>; OPS.len()],
    pub(super) panel: Panel,
    pub(super) log_view: Handle<MultilineEdit<Msg>>,
    pub(super) studio: Studio,
    pub(super) log: Vec<String>,
    /// The file the document came from or was last saved to.
    pub(super) path: Option<PathBuf>,
    /// The document as last saved or loaded, to show a modified mark.
    pub(super) saved: Document,
    pub(super) open_dialog: FileDialog<Msg>,
    pub(super) save_dialog: FileDialog<Msg>,
    /// Shared with scripted runs through [`Probe`].
    pub(super) preview: Rc<Cell<Preview>>,
    /// Identifies the lightmap in the picture, for scripted runs through [`Probe`].
    pub(super) light_key: Rc<Cell<u64>>,
    /// Everything but the menu and the status bar, while the level is showing.
    pub(super) work_host: Handle<xui::widget::Panel<Msg>>,
    pub(super) tex: Workspace,
}

impl Editor {
    /// Builds the window contents around `doc`.
    pub fn build(ui: &mut Ui<Msg>, doc: Document) -> xui::backend::Result<Editor> {
        Editor::build_from(ui, doc, None, None)
    }

    /// As [`Editor::build`] for a document read from `path`, with an optional
    /// first status message (e.g. why a file could not be opened).
    pub fn build_from(
        ui: &mut Ui<Msg>,
        doc: Document,
        path: Option<PathBuf>,
        message: Option<String>,
    ) -> xui::backend::Result<Editor> {
        let mut studio = Studio::new(&doc);
        studio.set_cache_level(path.as_deref());
        let saved = doc.clone();
        let mut initial = EditorState::new(doc);
        if let Some(m) = message {
            initial.message = m;
        }
        let state = Rc::new(RefCell::new(initial));
        let open_dialog = FileDialog::open_file(ui, "Open level")?
            .filter("Miniscent levels", &[files::EXT])
            .on_accept(|p| Some(Msg::OpenPath(p)))
            .on_cancel(|| None);
        let save_dialog = FileDialog::save_file(ui, "Save level")?
            .suggested_name("level.msl")
            .filter("Miniscent levels", &[files::EXT])
            .on_accept(|p| Some(Msg::SavePath(p)))
            .on_cancel(|| None);
        let tex = Workspace::new(studio.materials().clone());
        let mut app = Editor {
            state: state.clone(),
            classic: Arrangement::default(),
            quad: Arrangement::default(),
            disposition: Disposition::Quad,
            play: Handle::new(),
            menu: Handle::new(),
            status: Handle::new(),
            tools: Default::default(),
            ops: Default::default(),
            panel: Panel::default(),
            log_view: Handle::new(),
            studio,
            log: Vec::new(),
            path,
            saved,
            open_dialog,
            save_dialog,
            preview: Rc::new(Cell::new(Preview::Updating)),
            light_key: Rc::new(Cell::new(0)),
            work_host: Handle::new(),
            tex,
        };
        app.panel.attach(ui);
        app.tex.attach(ui);
        let (generation, table) = app.studio.materials().current();
        state.borrow_mut().texture_sizes = app
            .panel
            .show_materials(generation, table.ok().as_ref())
            .unwrap_or_default();
        let emit: Rc<dyn Fn(ViewEvent) -> Msg> = Rc::new(Msg::View);
        let views = stack().children((
            layouts::classic(&app.classic, &state, &emit).fill(1),
            layouts::quad(&app.quad, &state, &emit).fill(1),
        ));
        let root = ui.clone();
        let tool_button = |i: usize| {
            let (name, key, glyph, _) = TOOLS[i];
            let tip = format!("{name} ({})", key_name(key));
            let root = root.clone();
            build(move |ui| IconButton::new(ui, &root, glyph, &tip, Msg::Tool(i)))
                .bind(&app.tools[i])
        };
        let tools = column().gap(2).children((
            row()
                .gap(2)
                .children((tool_button(0), tool_button(1), tool_button(2))),
            row()
                .gap(2)
                .children((tool_button(3), tool_button(4), tool_button(5))),
            row()
                .gap(2)
                .children((tool_button(6), tool_button(7), tool_button(8))),
            row()
                .gap(2)
                .children((tool_button(9), tool_button(10), tool_button(11))),
        ));
        let op_button = |i: usize| {
            let (tip, glyph) = OPS[i];
            let root = root.clone();
            build(move |ui| IconButton::new(ui, &root, glyph, tip, Msg::NewOp(i))).bind(&app.ops[i])
        };
        let grid_names: Vec<String> = GRIDS.iter().map(|g| format!("{g}")).collect();
        let grid_refs: Vec<&str> = grid_names.iter().map(String::as_str).collect();
        let palette = scroll(column().padding(8).gap(8).children((
            group("Tools", tools),
            group(
                "New brush",
                row().gap(2).children((op_button(0), op_button(1))),
            ),
            group(
                "Grid size",
                column().child(combo_box(&grid_refs).on_select(Msg::Grid).then(|c| {
                    c.select(2);
                    c
                })),
            ),
        )));
        let side = column().gap(4).children((
            scroll(column().child(app.panel.layout())).fill(1),
            group(
                "Lighting log",
                column().child(multiline_edit().bind(&app.log_view).height(LOG_HEIGHT)),
            ),
        ));
        let work = row().children((
            palette.width(PALETTE_WIDTH),
            views.fill(1),
            side.width(SIDE_WIDTH),
        ));
        ui.root(
            column().children((
                menu_bar(menus).on_select(Msg::Menu).bind(&app.menu),
                stack()
                    .children((
                        panel(work).plain().bind(&app.work_host),
                        app.tex.layout(),
                        build(View3d::new).bind(&app.play),
                    ))
                    .fill(1),
                status_bar(&["Ready", "", "", ""]).bind(&app.status),
            )),
        )?;
        ui.on_key(shortcut);
        ui.every(16, Msg::Tick);
        app.play.get().set_visible(false);
        app.tex.set_visible(false);
        app.set_disposition(Disposition::Quad);
        app.log_view.get().set_enabled(false);
        app.refresh();
        app.show_prompt();
        Ok(app)
    }

    /// Repaints the views, refills the panel and rewrites the status bar.
    pub(super) fn refresh(&mut self) {
        for v in self.all_viewports() {
            v.get().invalidate();
        }
        self.panel.sync(&mut self.state.borrow_mut());
        let st = self.state.borrow();
        self.studio.set_highlights(st.highlighted_faces());
        let dirty = st.doc != self.saved;
        let bar = self.status.get();
        bar.set_text(0, &st.message);
        bar.set_text(
            2,
            &format!(
                "{} brushes  {} entities  grid {}{}",
                st.doc.brushes().len(),
                st.doc.entities().len(),
                st.grid,
                if dirty { "  [modified]" } else { "" }
            ),
        );
        let menu = self.menu.get();
        menu.set_enabled(cmd::UNDO, st.history.can_undo());
        menu.set_enabled(cmd::REDO, st.history.can_redo());
        let any = st.selection.is_some();
        let brush = st.selected_brush().is_some();
        menu.set_enabled(cmd::DELETE, any);
        for id in [cmd::FLIP, cmd::FORWARD, cmd::BACK, cmd::CARVE] {
            menu.set_enabled(id, brush);
        }
        menu.set_enabled(cmd::SLICE, brush && st.cut.is_some());
        menu.set_enabled(cmd::SLICE_SIDE, true);
        for id in [cmd::EXTRUDE, cmd::EXTRUDE_IN] {
            menu.set_enabled(id, !st.faces.is_empty());
        }
        for (i, t) in TOOLS.iter().enumerate() {
            self.tools[i].get().set_checked(t.3 == st.tool);
        }
        for (i, op) in [Op::Subtract, Op::Add].into_iter().enumerate() {
            self.ops[i].get().set_checked(st.new_op == op);
        }
        drop(st);
        self.sync_tex_menu();
    }

    /// Whether a text box has the keyboard focus, so letters and Delete are
    /// text there and not shortcuts.
    fn text_has_focus(&self, ui: &Ui<Msg>) -> bool {
        ui.focus_holder().is_some_and(|id| {
            self.panel.owns(id) || self.tex.owns(id) || id == self.log_view.get().id()
        })
    }

    /// The 3D pane's text until the first picture of a level arrives.
    pub(super) fn show_prompt(&self) {
        let text = format!(
            "Preparing the level...
F5 plays it.

{HELP}"
        );
        for arr in [&self.classic, &self.quad] {
            arr.view3d.get().show_text(&text);
        }
    }

    /// Adds lines to the log panel.
    pub(super) fn push_log(&mut self, lines: &[String]) {
        if lines.is_empty() {
            return;
        }
        for line in lines {
            self.log.extend(wrap(line, LOG_COLS));
        }
        let excess = self.log.len().saturating_sub(300);
        self.log.drain(..excess);
        // The box cannot be scrolled to its end, so it shows the newest lines.
        let from = self.log.len().saturating_sub(LOG_ROWS);
        self.log_view.get().set_text(&self.log[from..].join(
            "
",
        ));
    }

    pub(super) fn set_status(&self, text: &str) {
        self.status.get().set_text(0, text);
    }

    /// Says on the status bar how far the picture has caught up.
    fn show_preview(&self, preview: Preview) {
        if self.preview.replace(preview) != preview {
            self.status.get().set_text(3, preview.label());
        }
    }

    /// Opens the Instant Play layer over the whole work area.
    pub(super) fn begin_play(&mut self) {
        match self.studio.enter_play() {
            Ok(()) => {
                // Play covers the work area, and with the workspace up behind it
                // its shortcuts and its lit tab would go on.
                if self.tex.is_open() {
                    self.tex_close();
                }
                let play = self.play.get();
                play.set_visible(true);
                self.pane3d().set_grab(false);
                play.set_grab(true);
                play.focus();
                // Focus changes from the editor are not the player's input.
                play.take_events();
                let text = "Playing: Esc = back, WASD (ZQSD on AZERTY) move, Space fire, Q/E roll";
                self.state.borrow_mut().message = text.into();
                self.set_status(text);
            }
            Err(e) => {
                self.state.borrow_mut().message = e.clone();
                self.set_status(&e);
            }
        }
    }

    /// Back to the editor; the document is exactly as it was.
    pub(super) fn end_play(&mut self, message: Option<&str>) {
        self.studio.leave_play();
        self.play.get().set_grab(false);
        self.play.get().set_visible(false);
        self.pane3d().focus();
        if let Some(m) = message {
            self.state.borrow_mut().message = m.into();
        }
        self.refresh();
    }

    pub(super) fn refit(&self) {
        for v in self.all_viewports() {
            v.get().refit();
        }
    }
}

mod tick;

#[cfg(test)]
mod keys_tests;
#[cfg(test)]
mod layout_tests;
#[cfg(test)]
mod lock_tests;
#[cfg(test)]
mod material_tests;
#[cfg(test)]
mod tests;

impl App for Editor {
    type Msg = Msg;

    fn update(&mut self, msg: Msg, ui: &mut Ui<Msg>) {
        match msg {
            Msg::Tex(m) => {
                self.tex_message(m, ui);
                return;
            }
            Msg::Menu(cmd::TEX_OPEN) if self.studio.mode == Mode::Edit => self.tex_open(),
            Msg::Menu(id) if self.tex.is_open() => self.tex_menu(id, ui),
            Msg::Menu(cmd::LEVEL_SETTINGS) => self.level_settings(ui),
            Msg::Menu(id) => self.menu_command(id),
            Msg::Tool(i) => self
                .state
                .borrow_mut()
                .set_tool(TOOLS[i.min(TOOLS.len() - 1)].3),
            Msg::NewOp(i) => {
                self.state.borrow_mut().new_op = if i == 0 { Op::Subtract } else { Op::Add };
            }
            Msg::Grid(i) => self.state.borrow_mut().grid = GRIDS[i.min(GRIDS.len() - 1)],
            Msg::Face(m) => self.face_message(m),
            Msg::Prop(p) => {
                apply(&mut self.state.borrow_mut(), p);
            }
            Msg::View(ViewEvent::Hover(view, p)) => {
                let (a, b) = view.axes();
                let n = ["X", "Y", "Z"];
                self.status
                    .get()
                    .set_text(1, &format!("{} {:.2}   {} {:.2}", n[a], p[0], n[b], p[1]));
                return;
            }
            Msg::View(ViewEvent::Changed) => {}
            Msg::Tick | Msg::Step => {
                self.advance();
                return;
            }
            Msg::Guarded(inner) => {
                if self.studio.mode == Mode::Play || self.text_has_focus(ui) {
                    return;
                }
                // Of the single keys, only those about the recipe's history and its
                // blocks mean anything in the workspace.
                let about_recipe = matches!(*inner, Msg::Menu(cmd::UNDO | cmd::REDO | cmd::DELETE));
                if self.tex.is_open() && !about_recipe {
                    return;
                }
                if let Msg::Tool(i) = *inner
                    && self.pane3d().flies(TOOLS[i.min(TOOLS.len() - 1)].1)
                {
                    return;
                }
                self.update(*inner, ui);
                return;
            }
            Msg::Escape => {
                if self.studio.mode == Mode::Play {
                    self.end_play(Some("Back in the editor"));
                } else if self.tex.is_open() {
                    // A drag of a block is cancelled first; the next Esc goes back.
                    if !self.tex.cancel_drag() {
                        self.tex_close();
                    }
                } else {
                    let mut st = self.state.borrow_mut();
                    st.cancel_drag();
                    st.end_path();
                    drop(st);
                    self.refresh();
                }
                return;
            }
            Msg::OpenPath(p) => self.open_path(&p),
            Msg::SavePath(p) => self.save_path(p),
            Msg::Camera(cam) => self.studio.set_camera(*cam),
        }
        self.refresh();
    }
}
