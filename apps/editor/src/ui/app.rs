//! The editor application: menus, tool palette, viewports and the status
//! bar, wired to the shared [`EditorState`]. File, Build and Play commands
//! live in `commands.rs`.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::{Duration, Instant};

use mscent_world::Op;
use xui::prelude::*;
use xui::widget::{FileDialog, HasText, MenuId};
use xui::{Key, Modifiers};

use super::probe::Probe;
use super::props::{Panel, apply};
use super::view3d::View3d;
use super::viewport::{ViewEvent, Viewport};
use super::wrap::{LOG_COLS, LOG_ROWS, wrap};
use crate::files;
use crate::model::{Document, EditorState, EntityKind, Tool, View, state::GRIDS};
use crate::studio::{HELP, Mode, Studio, frame_size};

#[derive(Clone)]
pub enum Msg {
    Menu(MenuId),
    /// A tool palette button, by index into [`TOOLS`].
    Tool(usize),
    NewOp(usize),
    Grid(usize),
    View(ViewEvent),
    Prop(super::props::Prop),
    /// The frame timer: measures the elapsed time itself.
    Tick,
    /// Advance by this much time (scripted runs have no timers).
    Step(Duration),
    /// The Escape key anywhere: leaves Play.
    Escape,
    OpenPath(PathBuf),
    SavePath(PathBuf),
}

/// The palette: label and tool.
const TOOLS: [(&str, Tool); 6] = [
    ("Select / move", Tool::Select),
    ("Box brush", Tool::Box),
    ("Player start", Tool::Place(EntityKind::PlayerStart)),
    ("Light", Tool::Place(EntityKind::Light)),
    ("Enemy", Tool::Place(EntityKind::Enemy)),
    ("Goal", Tool::Place(EntityKind::Goal)),
];

/// Menu command ids.
pub mod cmd {
    use xui::widget::MenuId;
    pub const NEW: MenuId = MenuId::new(1);
    pub const OPEN: MenuId = MenuId::new(2);
    pub const SAVE: MenuId = MenuId::new(3);
    pub const BUILD: MenuId = MenuId::new(4);
    pub const PLAY: MenuId = MenuId::new(5);
    pub const SHOWCASE: MenuId = MenuId::new(6);
    pub const SAVE_AS: MenuId = MenuId::new(7);
    pub const UNDO: MenuId = MenuId::new(10);
    pub const REDO: MenuId = MenuId::new(11);
    pub const DELETE: MenuId = MenuId::new(12);
    pub const FLIP: MenuId = MenuId::new(13);
    pub const FORWARD: MenuId = MenuId::new(14);
    pub const BACK: MenuId = MenuId::new(15);
    pub const FIT: MenuId = MenuId::new(20);
    pub const RESET_CAMERA: MenuId = MenuId::new(21);
}

pub struct Editor {
    pub(super) state: Rc<RefCell<EditorState>>,
    pub(super) viewports: [Handle<Viewport<Msg>>; 3],
    pub(super) view3d: Handle<View3d<Msg>>,
    pub(super) play: Handle<View3d<Msg>>,
    menu: Handle<Menu<Msg>>,
    pub(super) status: Handle<StatusBar<Msg>>,
    tools: [Handle<ToggleButton<Msg>>; TOOLS.len()],
    op: Handle<RadioGroup<Msg>>,
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
    last_tick: Instant,
    last_stale: bool,
}

fn menus(m: &mut xui::widget::MenuScope<'_>) {
    m.submenu(MenuId::new(100), "&File", |f| {
        f.item(cmd::NEW, "&New   Ctrl+N");
        f.item(cmd::OPEN, "&Open...   Ctrl+O");
        f.item(cmd::SHOWCASE, "Open &showcase level");
        f.item(cmd::SAVE, "&Save   Ctrl+S");
        f.item(cmd::SAVE_AS, "Save &as...");
    });
    m.submenu(MenuId::new(101), "&Edit", |e| {
        e.item(cmd::UNDO, "&Undo   Ctrl+Z");
        e.item(cmd::REDO, "&Redo   Ctrl+Y");
        e.separator();
        e.item(cmd::DELETE, "&Delete   Del");
        e.item(cmd::FLIP, "&Flip add/subtract");
        e.item(cmd::FORWARD, "Move &later in order");
        e.item(cmd::BACK, "Move &earlier in order");
    });
    m.submenu(MenuId::new(102), "&View", |v| {
        v.item(cmd::FIT, "&Fit views to level");
        v.item(cmd::RESET_CAMERA, "&Reset 3D camera");
    });
    m.submenu(MenuId::new(103), "&Play", |p| {
        p.item(cmd::BUILD, "&Build   F7");
        p.item(cmd::PLAY, "&Play   F5");
    });
}

/// Global shortcuts: they work wherever the focus is.
fn shortcut(key: Key, mods: Modifiers) -> Option<Msg> {
    if mods.ctrl {
        return match key {
            Key::S => Some(Msg::Menu(cmd::SAVE)),
            Key::O => Some(Msg::Menu(cmd::OPEN)),
            Key::N => Some(Msg::Menu(cmd::NEW)),
            _ => None,
        };
    }
    match key {
        Key::F5 => Some(Msg::Menu(cmd::PLAY)),
        Key::F7 => Some(Msg::Menu(cmd::BUILD)),
        Key::ESCAPE => Some(Msg::Escape),
        _ => None,
    }
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
        let studio = Studio::new(&doc);
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
        let mut app = Editor {
            state: state.clone(),
            viewports: Default::default(),
            view3d: Handle::new(),
            play: Handle::new(),
            menu: Handle::new(),
            status: Handle::new(),
            tools: Default::default(),
            op: Handle::new(),
            panel: Panel::default(),
            log_view: Handle::new(),
            studio,
            log: Vec::new(),
            path,
            saved,
            open_dialog,
            save_dialog,
            last_tick: Instant::now(),
            last_stale: false,
        };
        app.panel.attach(ui);
        let emit: Rc<dyn Fn(ViewEvent) -> Msg> = Rc::new(Msg::View);
        let pane = |i: usize| {
            let (state, emit) = (state.clone(), emit.clone());
            let view = View::ALL[i];
            build(move |ui| Viewport::new(ui, view, state, emit)).bind(&app.viewports[i])
        };
        let views = column().gap(4).children((
            row()
                .gap(4)
                .children((pane(0).fill(1), pane(1).fill(1), pane(2).fill(1)))
                .fill(2),
            build(View3d::new).bind(&app.view3d).fill(3),
        ));
        let mut tools = column().gap(4);
        for (i, (name, _)) in TOOLS.iter().enumerate() {
            tools = tools.child(
                toggle_button(*name)
                    .checked(i == 0)
                    .on_toggle(move |_| Msg::Tool(i))
                    .bind(&app.tools[i]),
            );
        }
        let grid_names: Vec<String> = GRIDS.iter().map(|g| format!("{g}")).collect();
        let grid_refs: Vec<&str> = grid_names.iter().map(String::as_str).collect();
        let palette = scroll(
            column().padding(8).gap(8).children((
                group("Tools", tools),
                group(
                    "New brush",
                    column().child(
                        radio_group(&["Subtract", "Add"])
                            .selected(0)
                            .on_select(Msg::NewOp)
                            .bind(&app.op),
                    ),
                ),
                group(
                    "Grid size",
                    column().child(combo_box(&grid_refs).on_select(Msg::Grid).then(|c| {
                        c.select(2);
                        c
                    })),
                ),
            )),
        );
        let side = column().gap(4).children((
            scroll(column().child(app.panel.layout())).fill(1),
            group(
                "Build log",
                column().child(multiline_edit().bind(&app.log_view).height(150)),
            ),
        ));
        let work = row().children((palette.width(150), views.fill(1), side.width(280)));
        ui.root(
            column().children((
                menu_bar(menus).on_select(Msg::Menu).bind(&app.menu),
                stack()
                    .children((work, build(View3d::new).bind(&app.play)))
                    .fill(1),
                status_bar(&["Ready", "", ""]).bind(&app.status),
            )),
        )?;
        ui.on_key(shortcut);
        ui.every(16, Msg::Tick);
        app.play.get().set_visible(false);
        app.log_view.get().set_enabled(false);
        app.refresh();
        app.show_prompt();
        Ok(app)
    }

    /// Repaints the views, refills the panel and rewrites the status bar.
    pub(super) fn refresh(&mut self) {
        for v in &self.viewports {
            v.get().invalidate();
        }
        let st = self.state.borrow();
        self.panel.sync(&st);
        let stale = self.studio.stale(&st.doc);
        if stale != self.last_stale {
            self.last_stale = stale;
            self.studio.invalidate();
        }
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
        for id in [cmd::FLIP, cmd::FORWARD, cmd::BACK] {
            menu.set_enabled(id, brush);
        }
        for (i, (_, tool)) in TOOLS.iter().enumerate() {
            self.tools[i].get().set_checked(*tool == st.tool);
        }
        self.op.get().select(usize::from(st.new_op == Op::Add));
    }

    /// The 3D pane's text before the first build.
    pub(super) fn show_prompt(&self) {
        let text = format!(
            "No build yet.
Press F7 to build, F5 to play.

{HELP}"
        );
        self.view3d.get().show_text(&text);
    }

    /// Adds lines to the build log panel.
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

    /// Draws the studio's current picture into the pane that shows it.
    fn show_frame(&mut self) {
        let play = self.studio.mode == Mode::Play;
        let pane = if play {
            self.play.get()
        } else {
            self.view3d.get()
        };
        let (pw, ph) = pane.size();
        if pw == 0 || ph == 0 {
            return;
        }
        let (w, h) = frame_size((pw, ph), if play { 512 } else { 960 });
        let state = self.state.clone();
        let st = state.borrow();
        if let Some(fb) = self.studio.render(w, h, &st.doc) {
            pane.set_image(fb.pixels(), w, h);
        }
    }

    fn advance(&mut self, dt: Duration) {
        let pane = if self.studio.mode == Mode::Play {
            self.play.get()
        } else {
            self.view3d.get()
        };
        let events = pane.take_events();
        let tick = self.studio.tick(dt, &events);
        self.push_log(&tick.log);
        if let Some(s) = &tick.status {
            self.state.borrow_mut().message = s.clone();
            self.set_status(s);
        }
        if tick.play_now {
            self.begin_play();
        }
        if tick.left_play {
            self.end_play(None);
        }
        if tick.redraw || tick.play_now {
            self.show_frame();
        }
    }

    /// Opens the Instant Play layer over the whole work area.
    pub(super) fn begin_play(&mut self) {
        match self.studio.enter_play() {
            Ok(()) => {
                let play = self.play.get();
                play.set_visible(true);
                play.focus();
                // Focus changes from the editor are not the player's input.
                play.take_events();
                let text = "Playing: Esc = back, WASD move, Space fire, Q/E roll";
                self.state.borrow_mut().message = text.into();
                self.set_status(text);
                self.show_frame();
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
        self.play.get().set_visible(false);
        self.view3d.get().focus();
        if let Some(m) = message {
            self.state.borrow_mut().message = m.into();
        }
        self.show_frame();
        self.refresh();
    }

    pub(super) fn refit(&self) {
        for v in &self.viewports {
            v.get().refit();
        }
    }

    /// Handles for scripted runs and tests.
    pub fn probe(&self) -> Probe {
        Probe {
            state: self.state.clone(),
            play: self.play.get(),
        }
    }
}

#[cfg(test)]
mod tests;

impl App for Editor {
    type Msg = Msg;

    fn update(&mut self, msg: Msg, _ui: &mut Ui<Msg>) {
        match msg {
            Msg::Menu(id) => self.menu_command(id),
            Msg::Tool(i) => self.state.borrow_mut().tool = TOOLS[i].1,
            Msg::NewOp(i) => {
                self.state.borrow_mut().new_op = if i == 0 { Op::Subtract } else { Op::Add };
            }
            Msg::Grid(i) => self.state.borrow_mut().grid = GRIDS[i.min(GRIDS.len() - 1)],
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
            Msg::Tick => {
                let now = Instant::now();
                let dt = now
                    .duration_since(self.last_tick)
                    .min(Duration::from_millis(100));
                self.last_tick = now;
                self.advance(dt);
                return;
            }
            Msg::Step(dt) => {
                self.advance(dt);
                return;
            }
            Msg::Escape => {
                if self.studio.mode == Mode::Play {
                    self.end_play(Some("Back in the editor"));
                }
                return;
            }
            Msg::OpenPath(p) => self.open_path(&p),
            Msg::SavePath(p) => self.save_path(p),
        }
        self.refresh();
    }
}
