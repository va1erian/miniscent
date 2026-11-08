//! The script pane: the code editor, what is wrong with the script, basic
//! entities and samples on one tab, the guide on the other. It sits under the
//! views or in a window of its own. The text and its rules live in `script/`,
//! and the widgets in `script_widgets.rs`; here is what they share and where
//! they are.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use xui::app::WindowHandle;
use xui::backend::{PlatformSpec, WidgetId};
use xui::prelude::*;
use xui::widget::{HasText, ListView, Split};
use xui::{Dip, dip};
use xui_code_editor::{Editor as CodeEditor, Marker, MarkerKind};

use super::app::Msg;
use super::keys::shortcut;
use super::script_widgets::{GUIDE_TAB, PaneWidgets, Shared};
use super::wrap::wrap;
use crate::model::EditorState;
use crate::script::buffer::ScriptBuffer;
use crate::script::check::{Checker, Problem};

/// What the pane asks of the editor.
#[derive(Clone, Debug, PartialEq)]
pub enum ScriptMsg {
    /// The text changed in the code editor.
    Edited,
    /// A row of the problem list.
    Problem(usize),
    /// A basic entity's button, by index into `KIT`.
    Kit(usize),
    /// A sample, by index into `SAMPLES`.
    Sample(usize),
    /// The guide's example, by its number on the page, to put in the script.
    Example(usize),
    Reload,
    Save,
    Play,
    /// A page of the guide, by index.
    Topic(usize),
    /// In a window of its own, or under the views.
    Float(bool),
}

/// How tall the pane opens, and how small the views above can get.
const PANE_HEIGHT: i32 = 320;
const VIEWS_MIN: i32 = 120;

/// A divider past any window's end, for the split to pull back to its limit.
const FAR: f32 = 100_000.0;

/// How a problem wraps in its box.
const DETAIL_COLS: usize = 44;

/// The size the pane's own window opens at.
const WINDOW_SIZE: (f32, f32) = (980.0, 620.0);

/// The pane's own window has no app of its own: what happens there is the
/// editor's to deal with.
struct ScriptWindow {
    editor: Ui<Msg>,
}

impl App for ScriptWindow {
    type Msg = Msg;

    fn update(&mut self, msg: Msg, _ui: &mut Ui<Msg>) {
        self.editor.emit(msg);
    }
}

/// Keys in the pane's own window. A guarded key is a shortcut only outside a text
/// field, and all there is here to type in is one, which the editor cannot see
/// has the focus from its own window.
fn window_shortcut(key: xui::Key, mods: xui::Modifiers) -> Option<Msg> {
    shortcut(key, mods).filter(|msg| !matches!(msg, Msg::Guarded(_)))
}

pub struct ScriptPane {
    /// The set under the views, there from the start.
    docked: Rc<PaneWidgets>,
    /// The set in use: `docked` or the one in the window.
    active: Rc<RefCell<Rc<PaneWidgets>>>,
    window: Rc<RefCell<Option<WindowHandle<Msg>>>>,
    pub(super) split: Handle<Split<Msg>>,
    shared: Shared,
    pub checker: Checker,
    shown: Vec<Problem>,
    open: Rc<Cell<bool>>,
    /// In a window of its own, whether or not that is open right now.
    floats: bool,
    ui: Option<Ui<Msg>>,
}

impl ScriptPane {
    pub fn new(state: Rc<RefCell<EditorState>>) -> ScriptPane {
        let docked = Rc::new(PaneWidgets::default());
        ScriptPane {
            active: Rc::new(RefCell::new(docked.clone())),
            docked,
            window: Rc::default(),
            split: Handle::new(),
            shared: Shared {
                state,
                library: Rc::default(),
            },
            checker: Checker::default(),
            shown: Vec::new(),
            open: Rc::default(),
            floats: false,
            ui: None,
        }
    }

    /// The docked widgets, ready to mount under the views.
    pub fn layout(&self) -> Layout<Msg> {
        self.docked.layout(&self.shared, false)
    }

    /// What a built pane starts with.
    pub fn start(&mut self, ui: &Ui<Msg>) {
        self.ui = Some(ui.clone());
        self.docked.start(ui);
        self.show_problems();
        self.set_open(false);
    }

    fn widgets(&self) -> Rc<PaneWidgets> {
        self.active.borrow().clone()
    }

    pub fn code(&self) -> Rc<CodeEditor<Msg>> {
        self.widgets().code.get()
    }

    /// Shows or hides the pane, wherever it is. The views keep what it does not
    /// need.
    pub fn set_open(&mut self, open: bool) {
        self.open.set(open);
        if self.floats {
            self.show_docked(false);
            if open {
                self.open_window();
            } else {
                self.close_window();
            }
        } else {
            self.show_docked(open);
        }
    }

    fn show_docked(&self, shown: bool) {
        let split = self.split.get();
        let below = if shown { PANE_HEIGHT } else { 0 };
        split.set_min(Dip(VIEWS_MIN as f32), Dip(below as f32));
        split.set_position(Dip(FAR));
        split.relayout();
        self.docked.tabs.get().set_visible(shown);
    }

    pub fn is_open(&self) -> bool {
        self.open.get()
    }

    pub fn floats(&self) -> bool {
        self.floats
    }

    /// Moves the pane to its own window or under the views. It is on show after.
    pub fn set_floating(&mut self, floats: bool) {
        self.close_window();
        self.floats = floats;
        self.set_open(true);
        self.show_script();
    }

    /// Makes `widgets` the set in use: they get the text and what is known of it.
    /// The set not in use goes stale, so this is the only way to change which that
    /// is. The model's text has every edit still on its way to a widget in it.
    fn adopt(&mut self, widgets: Rc<PaneWidgets>) {
        let (line, col) = self.code().caret_line_col();
        *self.active.borrow_mut() = widgets;
        let text = {
            let mut st = self.shared.state.borrow_mut();
            st.script.take_widget_edits();
            st.script.text().to_string()
        };
        let code = self.code();
        code.set_text(&text);
        code.goto(line, col);
        self.show_problems();
    }

    fn open_window(&mut self) {
        if self.window.borrow().is_some() {
            return;
        }
        let Some(editor) = self.ui.clone() else {
            return;
        };
        let widgets = Rc::new(PaneWidgets::default());
        let (set, shared, forward_to) = (widgets.clone(), self.shared.clone(), editor.clone());
        let spec = PlatformSpec::new("Script").size(dip(WINDOW_SIZE.0), dip(WINDOW_SIZE.1));
        let opened = editor.open_window(spec, move |ui| {
            ui.on_key(window_shortcut);
            ui.on_close(|| Some(Msg::Script(ScriptMsg::Float(false))));
            if ui.root(set.layout(&shared, true)).is_err() {
                ui.close();
            }
            set.start(ui);
            ScriptWindow { editor: forward_to }
        });
        match opened {
            Ok(window) => {
                *self.window.borrow_mut() = Some(window);
                self.adopt(widgets);
            }
            Err(e) => {
                // Back under the views, so the script is not out of reach.
                self.floats = false;
                self.show_docked(true);
                self.shared.state.borrow_mut().message =
                    format!("Cannot open a window for the script: {e:?}");
            }
        }
    }

    fn close_window(&mut self) {
        if let Some(window) = self.window.borrow_mut().take() {
            window.close();
        }
        if !Rc::ptr_eq(&self.widgets(), &self.docked) {
            self.adopt(self.docked.clone());
        }
    }

    /// Play starts. A window of the pane's own goes away for it: the game takes
    /// the keyboard and the pointer, which the main window must hold to give.
    pub fn suspend(&mut self) {
        if self.floats {
            self.close_window();
        }
    }

    /// Play ended: back as it was.
    pub fn resume(&mut self) {
        if self.floats && self.open.get() {
            self.open_window();
        }
    }

    /// A page of the guide.
    pub fn show_topic(&self, topic: usize) {
        self.widgets().set_topic(topic);
    }

    pub fn show_guide(&self) {
        self.widgets().tabs.get().select(GUIDE_TAB);
    }

    /// The guide's `n`th example on the page on show.
    pub fn example(&self, n: usize) -> Option<String> {
        self.widgets().example(n)
    }

    pub fn show_script(&self) {
        let widgets = self.widgets();
        widgets.tabs.get().select(0);
        widgets.code.get().focus();
    }

    /// Whether one of the pane's editors holds the keys, so letters are text there
    /// and not shortcuts.
    pub fn owns(&self, id: WidgetId) -> bool {
        let widgets = self.widgets();
        widgets.code.get().id() == id || widgets.guide.get().id() == id
    }

    /// The zero-based line of a row of the problem list.
    pub fn problem_line(&self, row: usize) -> Option<usize> {
        self.shown.get(row).and_then(|p| p.line)
    }

    /// Back to the first row of the samples list, ready to pick the same again.
    pub fn reset_sample_list(&self) {
        self.widgets().samples.get().select(0);
    }

    /// Asks the checker to go on, and shows what it says.
    pub fn tick(&mut self, st: &EditorState) {
        if self.checker.tick(st.script.text(), &st.doc) {
            let report = self.checker.report();
            self.shown = report.problems.clone();
            *self.shared.library.borrow_mut() = report.library.clone();
            self.show_problems();
        }
    }

    /// Shows a whole problem, by its row in the list.
    pub fn show_detail(&self, row: usize) {
        let text = self.shown.get(row).map_or("", |p| p.text.as_str());
        let wrapped = wrap(text, DETAIL_COLS).join("\n");
        self.widgets().detail.get().set_text(&wrapped);
    }

    fn show_problems(&self) {
        self.show_detail(0);
        let widgets = self.widgets();
        let rows: Vec<String> = if self.shown.is_empty() {
            vec!["No problems".to_string()]
        } else {
            self.shown.iter().map(|p| p.text.clone()).collect()
        };
        widgets.problems.get().refresh_model(rows);
        let marks = self.shown.iter().filter_map(|p| {
            let kind = if p.warning {
                MarkerKind::Warning
            } else {
                MarkerKind::Error
            };
            p.line.map(|line| Marker::line(line, kind))
        });
        widgets.code.get().set_markers(marks.collect());
    }

    /// The line over the code: which file, and whether it is saved.
    pub fn show_state(&self, buffer: &ScriptBuffer, st: &EditorState) {
        let widgets = self.widgets();
        let text = status_text(buffer, st);
        let status = widgets.status.get();
        if status.text() != text {
            status.set_text(&text);
        }
        widgets.show_reload(buffer.conflict());
    }

    /// Handles for scripted runs.
    pub fn probe_handles(&self) -> ProbeHandles {
        ProbeHandles {
            active: self.active.clone(),
            window: self.window.clone(),
            open: self.open.clone(),
        }
    }
}

/// What a scripted run reads of the pane, which can change window under it.
pub struct ProbeHandles {
    active: Rc<RefCell<Rc<PaneWidgets>>>,
    window: Rc<RefCell<Option<WindowHandle<Msg>>>>,
    open: Rc<Cell<bool>>,
}

impl ProbeHandles {
    pub fn code(&self) -> Rc<CodeEditor<Msg>> {
        self.active.borrow().code.get()
    }

    pub fn problems(&self) -> Rc<ListView<Msg>> {
        self.active.borrow().problems.get()
    }

    pub fn is_open(&self) -> bool {
        self.open.get()
    }

    /// The title of the guide's page on show: its first block, the page's
    /// heading.
    pub fn guide_title(&self) -> Option<String> {
        let doc = self.active.borrow().guide.get().current_document();
        let first = doc.blocks().first()?;
        Some(doc.block_text(first).to_owned())
    }

    /// The pane's own window, if it is up.
    pub fn window(&self) -> Option<WindowHandle<Msg>> {
        self.window.borrow().clone()
    }
}

fn status_text(buffer: &ScriptBuffer, st: &EditorState) -> String {
    let file = st.doc.level_path().and_then(|p| {
        let script = p.with_extension("st");
        script.file_name().map(|n| n.to_string_lossy().into_owned())
    });
    if buffer.conflict() {
        return "The file changed on disk: Save overwrites it, Reload takes it".to_string();
    }
    match (file, buffer.dirty()) {
        (Some(name), true) => format!("{name}, modified"),
        (Some(name), false) if buffer.has_file() => name,
        (Some(name), false) => format!("{name}, not written yet: no script"),
        (None, _) if buffer.text().is_empty() => "No script yet".to_string(),
        (None, _) => "Not saved: save the level to keep this script".to_string(),
    }
}
