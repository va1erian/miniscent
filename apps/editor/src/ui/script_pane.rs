//! The script pane under the views: the code editor, what is wrong with the
//! script, basic entities and samples on one tab, the guide on the other. The
//! text and its rules live in `script/`; here are the widgets.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use xui::Dip;
use xui::backend::{Result as XuiResult, WidgetId};
use xui::prelude::*;
use xui::property::Font;
use xui::widget::{Fill, HasText, ListView, Run, Split, Tabs};
use xui_code_editor::{Editor as CodeEditor, FontConfig, Marker, MarkerKind, Options};

use super::app::Msg;
use super::script_lexer::{ScriptColours, ScriptCompleter};
use super::wrap::wrap;
use crate::model::EditorState;
use crate::script::buffer::ScriptBuffer;
use crate::script::check::{Checker, Problem};
use crate::script::guide::{self, Block};
use crate::script::kit::KIT;
use crate::script::samples::SAMPLES;
use crate::script::scan::Define;

/// What the pane asks of the editor.
#[derive(Clone, Debug, PartialEq)]
pub enum ScriptMsg {
    /// The text changed in the code editor.
    Edited,
    /// A row of the problem list.
    Problem(usize),
    /// A basic entity's button, by index into [`KIT`].
    Kit(usize),
    /// A sample, by index into [`SAMPLES`].
    Sample(usize),
    /// A guide example to put in the script.
    Insert(String),
    Reload,
    Save,
    Play,
    /// A guide page, by index.
    Topic(usize),
    Toggle,
    ShowGuide,
}

fn script_msg(m: ScriptMsg) -> Msg {
    Msg::Script(m)
}

/// How tall the pane opens, and how small the views above can get.
const PANE_HEIGHT: i32 = 320;
const VIEWS_MIN: i32 = 120;

/// A divider past any window's end, for the split to pull back to its limit.
const FAR: f32 = 100_000.0;

const GUIDE_TAB: usize = 1;

/// The box with a whole problem in it: its height, and how wide it wraps.
const DETAIL_HEIGHT: i32 = 70;
const DETAIL_COLS: usize = 44;

/// The monospace family the code is set in; a backend without it picks one.
const MONO: &str = "Consolas";

/// The container a guide page sits in.
type Host = xui::widget::Panel<Msg>;

pub struct ScriptPane {
    pub code: Handle<CodeEditor<Msg>>,
    pub(super) split: Handle<Split<Msg>>,
    tabs: Handle<Tabs<Msg>>,
    problems: Handle<ListView<Msg>>,
    /// A problem in full: a row of the list has room for little of it.
    detail: Handle<MultilineEdit<Msg>>,
    topics: Handle<ListView<Msg>>,
    status: Handle<Label<Msg>>,
    reload: Handle<Button<Msg>>,
    samples: Handle<ComboBox<Msg>>,
    pages: Vec<Handle<Host>>,
    pub checker: Checker,
    library: Rc<RefCell<Vec<Define>>>,
    shown: Vec<Problem>,
    open: Rc<Cell<bool>>,
    /// To show and hide what has no such method of its own.
    ui: Option<Ui<Msg>>,
}

impl Default for ScriptPane {
    fn default() -> Self {
        ScriptPane {
            code: Handle::new(),
            split: Handle::new(),
            tabs: Handle::new(),
            problems: Handle::new(),
            detail: Handle::new(),
            topics: Handle::new(),
            status: Handle::new(),
            reload: Handle::new(),
            samples: Handle::new(),
            pages: guide::pages().iter().map(|_| Handle::new()).collect(),
            checker: Checker::default(),
            library: Rc::default(),
            shown: Vec::new(),
            open: Rc::default(),
            ui: None,
        }
    }
}

fn mono(size: f32) -> Font {
    Font {
        family: Some(MONO.to_string()),
        size: Dip(size),
        bold: false,
        italic: false,
    }
}

fn code_editor(
    ui: &Ui<Msg>,
    state: Rc<RefCell<EditorState>>,
    library: Rc<RefCell<Vec<Define>>>,
) -> XuiResult<CodeEditor<Msg>> {
    let options = Options {
        font: FontConfig {
            family: Some(MONO.to_string()),
            size: Dip(13.0),
        },
        ..Options::default()
    };
    Ok(CodeEditor::with_options(ui, Default::default(), options)?
        .with_highlighter(ScriptColours)
        .with_completer(ScriptCompleter { state, library })
        .on_change(|_| Some(script_msg(ScriptMsg::Edited))))
}

fn paragraph(text: &str) -> Build<xui::widget::FlowText<Msg>, Msg> {
    let text = text.to_string();
    flow_text().then(move |f| f.run(Run::normal(text)))
}

fn heading(text: &str) -> Build<xui::widget::FlowText<Msg>, Msg> {
    let text = text.to_string();
    flow_text().then(move |f| f.run(Run::normal(text).weight(700).size(14.0)))
}

/// An example, a line at a time so it keeps its shape, and a button that puts
/// it in the script.
fn example(code: &str) -> Layout<Msg> {
    let mut lines = column().gap(0);
    for line in code.lines() {
        // A blank label has no height, and neither would the gap it leaves.
        let shown = if line.is_empty() { " " } else { line };
        lines = lines.child(label(shown).then(|l| l.font(mono(12.0))));
    }
    let insert = script_msg(ScriptMsg::Insert(format!("{code}\n")));
    column().gap(2).padding(4).children((
        lines,
        row().child(button("Insert in script").on_click(insert)),
    ))
}

fn page(blocks: &[Block]) -> Layout<Msg> {
    let mut out = column().padding(10).gap(8);
    for block in blocks {
        out = match block {
            Block::Heading(text) => out.child(heading(text)),
            Block::Text(text) => out.child(paragraph(text)),
            Block::Code(code) => out.child(example(code)),
            Block::Row(selector, help) => out.child(
                row().gap(10).children((
                    label(selector.as_str())
                        .then(|l| l.font(mono(12.0)))
                        .width(220),
                    label(help.as_str()).fill(1),
                )),
            ),
        };
    }
    out
}

impl ScriptPane {
    /// The widgets, ready to mount under the views.
    pub fn layout(&self, state: &Rc<RefCell<EditorState>>) -> Layout<Msg> {
        let tabs = tabs()
            .page("Script", self.script_tab(state))
            .page("Guide", self.guide_tab())
            .bind(&self.tabs);
        column().children((tabs.fill(1),))
    }

    fn script_tab(&self, state: &Rc<RefCell<EditorState>>) -> Layout<Msg> {
        let (state, library) = (state.clone(), self.library.clone());
        let editor = build(move |ui| code_editor(ui, state.clone(), library.clone()))
            .bind(&self.code)
            .fill(1);
        let sample_rows: Vec<String> = std::iter::once("Open a sample...".to_string())
            .chain(SAMPLES.iter().map(|s| s.title.to_string()))
            .collect();
        let sample_refs: Vec<&str> = sample_rows.iter().map(String::as_str).collect();
        let bar = row().gap(6).children((
            button("Save").on_click(script_msg(ScriptMsg::Save)),
            button("Play").on_click(script_msg(ScriptMsg::Play)),
            button("Reload")
                .on_click(script_msg(ScriptMsg::Reload))
                .bind(&self.reload),
            label("").bind(&self.status).fill(1),
            combo_box(&sample_refs)
                .on_select(|row| {
                    script_msg(if row == 0 {
                        ScriptMsg::Sample(usize::MAX)
                    } else {
                        ScriptMsg::Sample(row - 1)
                    })
                })
                .bind(&self.samples)
                .width(300),
        ));
        let problems = list()
            .column("Problems", Fill)
            .then(|l| l.on_select(|row| Some(script_msg(ScriptMsg::Problem(row)))))
            .bind(&self.problems);
        let mut kit = column().gap(3);
        for (i, entry) in KIT.iter().enumerate() {
            kit = kit.child(button(entry.label).on_click(script_msg(ScriptMsg::Kit(i))));
        }
        row().gap(6).padding(4).children((
            column().gap(4).children((bar, editor)).fill(1),
            column()
                .gap(4)
                .children((
                    problems.fill(1),
                    multiline_edit().bind(&self.detail).height(DETAIL_HEIGHT),
                ))
                .width(290),
            column()
                .children((group(
                    "Add entity",
                    column().children((scroll(column().padding(2).child(kit)).fill(1),)),
                )
                .fill(1),))
                .width(130),
        ))
    }

    fn guide_tab(&self) -> Layout<Msg> {
        let pages = guide::pages();
        let mut stacked = stack();
        for (i, p) in pages.iter().enumerate() {
            let content = column().children((scroll(page(&p.blocks)).fill(1),));
            stacked = stacked.child(panel(content).plain().bind(&self.pages[i]).fill(1));
        }
        let topics = list()
            .column("Topics", Fill)
            .then(|l| l.on_select(|row| Some(script_msg(ScriptMsg::Topic(row)))))
            .bind(&self.topics);
        row().gap(6).padding(4).children((
            column().children((topics.fill(1),)).width(170),
            stacked.fill(1),
        ))
    }

    /// What a built pane starts with.
    pub fn start(&mut self, ui: &Ui<Msg>) {
        self.ui = Some(ui.clone());
        let titles: Vec<String> = guide::pages().into_iter().map(|p| p.title).collect();
        self.topics.get().set_model(titles);
        self.topics.get().select(Some(0));
        self.show_topic(ui, 0);
        self.show_problems();
        self.detail.get().set_enabled(false);
        ui.set_visible(self.reload.get().id(), false);
        self.set_open(false);
    }

    /// Shows or hides the pane. The views keep what it does not need.
    pub fn set_open(&mut self, open: bool) {
        self.open.set(open);
        let split = self.split.get();
        let below = if open { PANE_HEIGHT } else { 0 };
        split.set_min(Dip(VIEWS_MIN as f32), Dip(below as f32));
        split.set_position(Dip(FAR));
        split.relayout();
        self.tabs.get().set_visible(open);
    }

    pub fn is_open(&self) -> bool {
        self.open.get()
    }

    /// Whether the pane is on show, for scripted runs.
    pub fn open_flag(&self) -> Rc<Cell<bool>> {
        self.open.clone()
    }

    /// The problem list, for scripted runs.
    pub fn problems(&self) -> Rc<ListView<Msg>> {
        self.problems.get()
    }

    /// The page of the guide on show.
    pub fn show_topic(&self, ui: &Ui<Msg>, topic: usize) {
        for (i, handle) in self.pages.iter().enumerate() {
            ui.set_visible(handle.get().id(), i == topic);
        }
    }

    pub fn show_guide(&self) {
        self.tabs.get().select(GUIDE_TAB);
    }

    pub fn show_script(&self) {
        self.tabs.get().select(0);
        self.code.get().focus();
    }

    /// Whether the pane's code editor holds the keys, so letters are text there
    /// and not shortcuts.
    pub fn owns(&self, id: WidgetId) -> bool {
        self.code.get().id() == id
    }

    /// The zero-based line of a row of the problem list.
    pub fn problem_line(&self, row: usize) -> Option<usize> {
        self.shown.get(row).and_then(|p| p.line)
    }

    /// Back to the first row of the samples list, ready to pick the same again.
    pub fn reset_sample_list(&self) {
        self.samples.get().select(0);
    }

    /// Asks the checker to go on, and shows what it says.
    pub fn tick(&mut self, st: &EditorState) {
        if self.checker.tick(st.script.text(), &st.doc) {
            let report = self.checker.report();
            self.shown = report.problems.clone();
            *self.library.borrow_mut() = report.library.clone();
            self.show_problems();
        }
    }

    /// Shows a whole problem, by its row in the list.
    pub fn show_detail(&self, row: usize) {
        let text = self.shown.get(row).map_or("", |p| p.text.as_str());
        self.detail
            .get()
            .set_text(&wrap(text, DETAIL_COLS).join("\n"));
    }

    fn show_problems(&self) {
        self.show_detail(0);
        let rows: Vec<String> = if self.shown.is_empty() {
            vec!["No problems".to_string()]
        } else {
            self.shown.iter().map(|p| p.text.clone()).collect()
        };
        self.problems.get().refresh_model(rows);
        let marks = self.shown.iter().filter_map(|p| {
            let kind = if p.warning {
                MarkerKind::Warning
            } else {
                MarkerKind::Error
            };
            p.line.map(|line| Marker::line(line, kind))
        });
        self.code.get().set_markers(marks.collect());
    }

    /// The line over the code: which file, and whether it is saved.
    pub fn show_state(&self, buffer: &ScriptBuffer, st: &EditorState) {
        let conflict = buffer.conflict();
        let text = status_text(buffer, st);
        let status = self.status.get();
        if status.text() != text {
            status.set_text(&text);
        }
        if let Some(ui) = &self.ui {
            ui.set_visible(self.reload.get().id(), conflict);
        }
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
