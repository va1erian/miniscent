//! The widgets of the script pane, one set for each place it can be: under the
//! views, or in its own window. A widget cannot move between windows, so moving
//! the pane makes a new set over the same model.
//!
//! A widget costs on every re-flow of its window, hidden or not, and a label
//! changing its text asks for one. A page of wrapped text as separate widgets took
//! 100 ms a re-flow, so the guide is one code editor, which measures nothing.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use xui::Dip;
use xui::backend::Result as XuiResult;
use xui::prelude::*;
use xui::widget::{Fill, ListView, Tabs};
use xui_code_editor::{
    Editor as CodeEditor, FontConfig, Highlighter, LineState, Options, Token, TokenClass,
};

use super::app::Msg;
use super::script_lexer::{ScriptColours, ScriptCompleter};
use super::script_pane::ScriptMsg;
use super::wrap::wrap;
use crate::model::EditorState;
use crate::script::guide::{self, Block, Page};
use crate::script::kit::KIT;
use crate::script::samples::SAMPLES;
use crate::script::scan::Define;

pub(super) const GUIDE_TAB: usize = 1;

/// The box with a whole problem in it.
const DETAIL_HEIGHT: i32 = 70;

/// The monospace family the code is set in; a backend without it picks one.
const MONO: &str = "Consolas";

/// Characters per line of the guide's text, which fits the pane's own window.
const GUIDE_COLS: usize = 96;

/// How an example is set apart in the guide's text, and how the colours know it.
const CODE_INDENT: &str = "    ";

/// Width of the first column of the reference.
const SELECTOR_COLS: usize = 28;

/// What the widgets are built over.
#[derive(Clone)]
pub(super) struct Shared {
    pub state: Rc<RefCell<EditorState>>,
    pub library: Rc<RefCell<Vec<Define>>>,
}

fn script_msg(m: ScriptMsg) -> Msg {
    Msg::Script(m)
}

/// A page of the guide as text, and where its examples are in it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct RenderedPage {
    pub text: String,
    /// The zero-based lines each example takes, end excluded, and its code.
    pub examples: Vec<(usize, usize, String)>,
}

impl RenderedPage {
    /// The example on `line`, or else the next one down, or else the last: what
    /// someone means who has scrolled to one and pressed the button.
    pub fn example_at(&self, line: usize) -> Option<&str> {
        let found = self.examples.iter().find(|(_, end, _)| line < *end);
        let chosen = found.or(self.examples.last());
        chosen.map(|(_, _, code)| code.as_str())
    }
}

/// Lays a page out as text: wrapped paragraphs, examples indented.
pub(super) fn render_page(page: &Page) -> RenderedPage {
    let mut lines: Vec<String> = vec![format!("# {}", page.title), String::new()];
    let mut examples = Vec::new();
    for block in &page.blocks {
        match block {
            Block::Heading(text) => {
                lines.push(format!("## {text}"));
                lines.push(String::new());
            }
            Block::Text(text) => {
                lines.extend(wrap(text, GUIDE_COLS));
                lines.push(String::new());
            }
            Block::Code(code) => {
                let start = lines.len();
                lines.extend(code.lines().map(|l| format!("{CODE_INDENT}{l}")));
                examples.push((start, lines.len(), format!("{code}\n")));
                lines.push(String::new());
            }
            Block::Row(selector, help) => {
                lines.push(format!("  {selector:<SELECTOR_COLS$}{help}"));
            }
        }
    }
    RenderedPage {
        text: lines.join("\n"),
        examples,
    }
}

/// Headings stand out, and examples are coloured as what they are.
struct GuideColours;

impl Highlighter for GuideColours {
    fn lex_line(&self, line: &str, _state: &LineState) -> (Vec<Token>, LineState) {
        let plain = LineState::from_raw(0);
        if line.starts_with('#') {
            let token = Token {
                class: TokenClass::Keyword,
                start: 0,
                end: line.chars().count(),
            };
            return (vec![token], plain);
        }
        let Some(code) = line.strip_prefix(CODE_INDENT) else {
            return (Vec::new(), plain);
        };
        let (mut tokens, _) = ScriptColours.lex_line(code, &plain);
        let shift = CODE_INDENT.len();
        for token in &mut tokens {
            token.start += shift;
            token.end += shift;
        }
        (tokens, plain)
    }
}

#[derive(Default)]
pub struct PaneWidgets {
    pub code: Handle<CodeEditor<Msg>>,
    pub(super) guide: Handle<CodeEditor<Msg>>,
    pub(super) tabs: Handle<Tabs<Msg>>,
    pub(super) problems: Handle<ListView<Msg>>,
    pub(super) detail: Handle<MultilineEdit<Msg>>,
    topics: Handle<ListView<Msg>>,
    pub(super) status: Handle<Label<Msg>>,
    pub(super) reload: Handle<Button<Msg>>,
    pub(super) samples: Handle<ComboBox<Msg>>,
    /// The guide's page on show.
    topic: Cell<usize>,
    page: RefCell<RenderedPage>,
    ui: RefCell<Option<Ui<Msg>>>,
}

fn editor_options() -> Options {
    Options {
        font: FontConfig {
            family: Some(MONO.to_string()),
            size: Dip(13.0),
        },
        ..Options::default()
    }
}

fn code_editor(ui: &Ui<Msg>, shared: Shared) -> XuiResult<CodeEditor<Msg>> {
    let completer = ScriptCompleter {
        state: shared.state,
        library: shared.library,
    };
    Ok(
        CodeEditor::with_options(ui, Default::default(), editor_options())?
            .with_highlighter(ScriptColours)
            .with_completer(completer)
            .on_change(|_| Some(script_msg(ScriptMsg::Edited))),
    )
}

/// The guide's page. The widget has no read-only mode, so a change is put back.
fn guide_viewer(ui: &Ui<Msg>) -> XuiResult<CodeEditor<Msg>> {
    Ok(
        CodeEditor::with_options(ui, Default::default(), editor_options())?
            .with_highlighter(GuideColours)
            .on_change(|_| Some(script_msg(ScriptMsg::GuideEdited))),
    )
}

impl PaneWidgets {
    /// The widgets, ready to mount. `floating` says which way the button moves it.
    pub(super) fn layout(&self, shared: &Shared, floating: bool) -> Layout<Msg> {
        let tabs = tabs()
            .page("Script", self.script_tab(shared, floating))
            .page("Guide", self.guide_tab())
            .bind(&self.tabs);
        column().children((tabs.fill(1),))
    }

    fn bar(&self, floating: bool) -> Layout<Msg> {
        let sample_rows: Vec<String> = std::iter::once("Open a sample...".to_string())
            .chain(SAMPLES.iter().map(|s| s.title.to_string()))
            .collect();
        let sample_refs: Vec<&str> = sample_rows.iter().map(String::as_str).collect();
        let (place, moved) = if floating {
            ("Dock", ScriptMsg::Float(false))
        } else {
            ("Own window", ScriptMsg::Float(true))
        };
        row().gap(6).children((
            button("Save").on_click(script_msg(ScriptMsg::Save)),
            button("Play").on_click(script_msg(ScriptMsg::Play)),
            button("Reload")
                .on_click(script_msg(ScriptMsg::Reload))
                .bind(&self.reload),
            label("").bind(&self.status).fill(1),
            button(place).on_click(script_msg(moved)),
            combo_box(&sample_refs)
                .on_select(|row| {
                    script_msg(if row == 0 {
                        ScriptMsg::Sample(usize::MAX)
                    } else {
                        ScriptMsg::Sample(row - 1)
                    })
                })
                .bind(&self.samples)
                .width(240),
        ))
    }

    fn script_tab(&self, shared: &Shared, floating: bool) -> Layout<Msg> {
        let shared = shared.clone();
        let editor = build(move |ui| code_editor(ui, shared.clone()))
            .bind(&self.code)
            .fill(1);
        let problems = list()
            .column("Problems", Fill)
            .then(|l| l.on_select(|row| Some(script_msg(ScriptMsg::Problem(row)))))
            .bind(&self.problems);
        let mut kit = column().gap(3);
        for (i, entry) in KIT.iter().enumerate() {
            kit = kit.child(button(entry.label).on_click(script_msg(ScriptMsg::Kit(i))));
        }
        row().gap(6).padding(4).children((
            column()
                .gap(4)
                .children((self.bar(floating), editor))
                .fill(1),
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
        let topics = list()
            .column("Topics", Fill)
            .then(|l| l.on_select(|row| Some(script_msg(ScriptMsg::Topic(row)))))
            .bind(&self.topics);
        let insert =
            button("Insert example in script").on_click(script_msg(ScriptMsg::InsertExample));
        row().gap(6).padding(4).children((
            column()
                .gap(4)
                .children((topics.fill(1), insert))
                .width(190),
            build(guide_viewer).bind(&self.guide).fill(1),
        ))
    }

    /// What newly built widgets start with.
    pub(super) fn start(&self, ui: &Ui<Msg>) {
        *self.ui.borrow_mut() = Some(ui.clone());
        let titles: Vec<String> = guide::pages().into_iter().map(|p| p.title).collect();
        self.topics.get().set_model(titles);
        self.topics.get().select(Some(0));
        self.set_topic(0);
        self.detail.get().set_enabled(false);
        self.show_reload(false);
    }

    pub(super) fn show_reload(&self, shown: bool) {
        if let Some(ui) = self.ui.borrow().as_ref() {
            ui.set_visible(self.reload.get().id(), shown);
        }
    }

    /// Shows a page of the guide. One past the end shows nothing new.
    pub(super) fn set_topic(&self, topic: usize) {
        if let Some(page) = guide::pages().get(topic) {
            self.topic.set(topic);
            let rendered = render_page(page);
            self.guide.get().set_text(&rendered.text);
            *self.page.borrow_mut() = rendered;
        }
    }

    /// The guide cannot be edited: what a key did there is put back.
    pub(super) fn restore_guide(&self) {
        let (line, col) = self.guide.get().caret_line_col();
        self.guide.get().set_text(&self.page.borrow().text);
        self.guide.get().goto(line, col);
    }

    /// The example the guide's caret is in or above.
    pub(super) fn example_here(&self) -> Option<String> {
        let (line, _) = self.guide.get().caret_line_col();
        self.page.borrow().example_at(line).map(str::to_string)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page(blocks: Vec<Block>) -> Page {
        Page {
            title: "T".into(),
            blocks,
        }
    }

    #[test]
    fn a_page_is_text_and_knows_where_its_examples_are() {
        let rendered = render_page(&page(vec![
            Block::Text("Hello.".into()),
            Block::Code("x := 1.\ny := 2.".into()),
            Block::Heading("Next".into()),
            Block::Code("z := 3.".into()),
        ]));
        let lines: Vec<&str> = rendered.text.lines().collect();
        assert_eq!(lines[0], "# T");
        assert_eq!(lines[2], "Hello.");
        assert_eq!(lines[4], "    x := 1.");
        assert_eq!(lines[7], "## Next");
        assert_eq!(
            rendered.examples[0],
            (4, 6, "x := 1.\ny := 2.\n".to_string())
        );
        assert_eq!(rendered.examples[1].2, "z := 3.\n");
    }

    #[test]
    fn the_example_meant_is_the_one_under_the_caret_or_the_next() {
        let rendered = render_page(&page(vec![
            Block::Code("a.".into()),
            Block::Text("Between.".into()),
            Block::Code("b.".into()),
        ]));
        assert_eq!(rendered.example_at(0), Some("a.\n"), "above the first");
        assert_eq!(rendered.example_at(2), Some("a.\n"), "in it");
        assert_eq!(rendered.example_at(4), Some("b.\n"), "above the next");
        assert_eq!(rendered.example_at(999), Some("b.\n"), "past them all");
        assert_eq!(render_page(&page(vec![])).example_at(0), None);
    }

    #[test]
    fn long_text_wraps_and_every_page_fits() {
        for page in guide::pages() {
            let rendered = render_page(&page);
            for line in rendered.text.lines() {
                let is_example = line.starts_with(CODE_INDENT);
                assert!(
                    is_example || line.chars().count() <= GUIDE_COLS + 40,
                    "{}: {line}",
                    page.title
                );
            }
        }
    }

    #[test]
    fn the_reference_lists_every_message() {
        let pages = guide::pages();
        let rendered = render_page(pages.last().expect("a reference page"));
        for entry in crate::script::api::API {
            assert!(rendered.text.contains(entry.selector), "{}", entry.selector);
        }
    }

    #[test]
    fn examples_are_coloured_with_their_columns_in_the_line() {
        let plain = LineState::from_raw(0);
        let (tokens, _) = GuideColours.lex_line("    x := 1.", &plain);
        assert_eq!((tokens[0].start, tokens[0].end), (4, 5));
        assert_eq!(GuideColours.lex_line("Just words.", &plain).0, []);
        let (heading, _) = GuideColours.lex_line("# Title", &plain);
        assert_eq!(heading[0].class, TokenClass::Keyword);
    }
}
