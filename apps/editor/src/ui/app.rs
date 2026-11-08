//! The editor application: menus, tool palette, viewports and the status
//! bar, wired to the shared [`EditorState`]. File and Play commands live in
//! `commands.rs`. The 3D pane follows the document by itself: see
//! `studio/preview.rs`.

use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;

use mscent_world::Op;
use xui::prelude::*;
use xui::widget::{Dialog, FileDialog, HasText};

pub use super::commands::cmd;
use super::commands::menus;
use super::keys::{OPS, TOOLS, key_name, shortcut};
use super::layouts::{self, Arrangement, Disposition};
use super::level_dialog::FogWindow;
use super::manual_window::ManualMsg;
pub use super::messages::Msg;
use super::props::{Panel, apply};
use super::script_pane::{ScriptMsg, ScriptPane};
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
    pub(super) script: ScriptPane,
    /// Asks before unsaved script edits go. Not a modal window: the default
    /// backend has no modal loop, and `open_modal` returns at once with nothing.
    pub(super) discard_dialog: Dialog<Msg>,
    /// What is waiting on that answer.
    pub(super) after_discard: Option<Msg>,
    /// The level settings window, once opened. It may have closed itself since.
    pub(super) fog_window: Rc<RefCell<Option<FogWindow>>>,
    /// The manual's window, once opened. It may have been closed since.
    pub(super) manual: Rc<RefCell<Option<super::manual_window::Manual>>>,
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
    /// When the 3D camera's mark last moved in the 2D views.
    pub(super) eye_moved: std::time::Instant,
    /// Everything but the menu and the status bar, while the level is showing.
    pub(super) work_host: Handle<xui::widget::Panel<Msg>>,
    pub(super) tex: Workspace,
    /// The frame tick's beat and `--live-timing`'s script.
    pub(super) clock: super::live::Clock,
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
        let discard_dialog = Dialog::confirm(
            ui,
            "Discard unsaved script edits?",
            "The script has edits that are not saved. Discard them?",
        )?
        .on_action(|action| Some(Msg::Discard(action)));
        let tex = Workspace::new(studio.materials().clone());
        let mut app = Editor {
            state: state.clone(),
            classic: Arrangement::default(),
            quad: Arrangement::default(),
            disposition: Disposition::Quad,
            play: Handle::new(),
            script: ScriptPane::new(state.clone()),
            discard_dialog,
            after_discard: None,
            fog_window: Rc::default(),
            manual: Rc::default(),
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
            eye_moved: std::time::Instant::now(),
            work_host: Handle::new(),
            tex,
            clock: super::live::Clock::default(),
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
            row().gap(2).children((tool_button(12), tool_button(13))),
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
                        panel(
                            column().children((split(
                                column().children((work.fill(1),)),
                                app.script.layout(),
                            )
                            .stacked()
                            .bind(&app.script.split)
                            .fill(1),)),
                        )
                        .plain()
                        .bind(&app.work_host),
                        app.tex.layout(),
                        app.tex.new_form.layout(),
                        build(View3d::new).bind(&app.play),
                    ))
                    .fill(1),
                status_bar(&["Ready", "", "", ""]).bind(&app.status),
            )),
        )?;
        ui.on_key(shortcut);
        ui.on_close(|| Some(Msg::CloseRequested));
        app.play.get().set_visible(false);
        app.tex.set_visible(false);
        app.set_disposition(Disposition::Quad);
        app.log_view.get().set_enabled(false);
        app.script.start(ui);
        app.state.borrow_mut().open_script();
        app.sync_script();
        if !app.state.borrow().script.text().is_empty() {
            app.script.set_open(true);
        }
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
        self.studio.set_highlights(st.highlights());
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

    /// A step of a drag in a 2D view or of a sculpt stroke: only the views, the
    /// selected brush's bounds and the 3D highlight change. A full refresh sets
    /// every panel, menu item and tool button, and each repaints its part of the
    /// window, which made a drag step cost 25 to 60 ms; the release does that once.
    pub(super) fn refresh_gesture(&mut self) {
        for v in self.shown_viewports() {
            v.get().invalidate();
        }
        let st = self.state.borrow();
        self.panel.sync_bounds(&st);
        self.studio.set_highlights(st.highlights());
    }

    /// Whether a text box has the keyboard focus, so letters and Delete are
    /// text there and not shortcuts.
    fn text_has_focus(&self, ui: &Ui<Msg>) -> bool {
        ui.focus_holder().is_some_and(|id| {
            self.panel.owns(id)
                || self.tex.owns(id)
                || self.script.owns(id)
                || id == self.log_view.get().id()
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
        if self.ask_before(&msg) {
            return;
        }
        match msg {
            Msg::Tex(m) => {
                self.tex_message(m, ui);
                return;
            }
            Msg::Menu(cmd::TEX_OPEN) if self.studio.mode == Mode::Edit => self.tex_open(),
            Msg::Menu(id) if self.tex.is_open() => self.tex_menu(id, ui),
            Msg::Discard(action) => {
                if let Some(next) = self.discard_answer(action) {
                    self.update(next, ui);
                }
                return;
            }
            Msg::Menu(cmd::LEVEL_SETTINGS) => self.level_settings(ui),
            Msg::Menu(cmd::MANUAL) => return self.manual_message(ManualMsg::Open, ui),
            Msg::Manual(m) => return self.manual_message(m, ui),
            Msg::SetFog(start, end) => self.set_fog(start, end),
            Msg::CloseRequested => {
                ui.quit();
                return;
            }
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
            // No refresh: nothing in the level changed, and this is every key.
            Msg::Script(ScriptMsg::Edited) => {
                self.script_message(ScriptMsg::Edited);
                return;
            }
            Msg::Script(m) => self.script_message(m),
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
            Msg::View(ViewEvent::Changed) if self.state.borrow().drag.is_some() => {
                self.refresh_gesture();
                return;
            }
            Msg::View(ViewEvent::Changed) => {}
            Msg::Tick | Msg::Step => {
                super::meter::time(super::meter::Lane::Tick, || self.advance());
                return;
            }
            Msg::Live => return self.live_message(ui),
            Msg::Guarded(inner) => {
                // Enter answers what the workspace has on top, from the name field too.
                if matches!(*inner, Msg::Menu(cmd::SLICE))
                    && self.tex.is_open()
                    && self.tex_answer_key(true, ui)
                {
                    return;
                }
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
                    // What is on top goes first: the question or the form, then a
                    // drag of a block, and the next Esc goes back.
                    if !self.tex_answer_key(false, ui) && !self.tex.cancel_drag() {
                        self.tex_close();
                    }
                } else {
                    let mut st = self.state.borrow_mut();
                    st.cancel_drag();
                    st.end_path();
                    drop(st);
                    self.refresh();
                }
                self.drop_picked_kit();
                return;
            }
            Msg::OpenPath(p) => self.open_path(&p),
            Msg::SavePath(p) => self.save_path(p),
            Msg::Camera(cam) => self.studio.set_camera(*cam),
        }
        self.refresh();
    }
}
