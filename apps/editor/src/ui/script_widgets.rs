//! The widgets of the script pane, one set for each place it can be: under the
//! views, or in its own window. A widget cannot move between windows, so moving
//! the pane makes a new set over the same model.
//!
//! A widget costs on every re-flow of its window, hidden or not, and a label
//! changing its text asks for one. A page of wrapped text as separate widgets took
//! 100 ms a re-flow, so the guide is one HTML view, which measures nothing: the
//! page is laid out on the view's own thread, and only when it is on show.

use std::cell::RefCell;
use std::rc::Rc;

use xui::Dip;
use xui::backend::Result as XuiResult;
use xui::geometry::Rect;
use xui::prelude::*;
use xui::widget::{Fill, ListView, Tabs};
use xui_blitz::{BlitzView, BlitzViewEvent};
use xui_code_editor::{Editor as CodeEditor, FontConfig, Options};

use super::app::Msg;
use super::guide_html;
use super::script_lexer::{ScriptColours, ScriptCompleter};
use super::script_pane::ScriptMsg;
use crate::model::EditorState;
use crate::script::guide;
use crate::script::kit::KIT;
use crate::script::samples::SAMPLES;
use crate::script::scan::Define;

pub(super) const GUIDE_TAB: usize = 1;

/// The box with a whole problem in it.
const DETAIL_HEIGHT: i32 = 70;

/// The monospace family the code is set in; a backend without it picks one.
const MONO: &str = "Consolas";

/// What the widgets are built over.
#[derive(Clone)]
pub(super) struct Shared {
    pub state: Rc<RefCell<EditorState>>,
    pub library: Rc<RefCell<Vec<Define>>>,
}

fn script_msg(m: ScriptMsg) -> Msg {
    Msg::Script(m)
}

#[derive(Default)]
pub struct PaneWidgets {
    pub code: Handle<CodeEditor<Msg>>,
    pub(super) guide: Handle<BlitzView<Msg>>,
    pub(super) tabs: Handle<Tabs<Msg>>,
    pub(super) problems: Handle<ListView<Msg>>,
    pub(super) detail: Handle<MultilineEdit<Msg>>,
    topics: Handle<ListView<Msg>>,
    pub(super) status: Handle<Label<Msg>>,
    pub(super) reload: Handle<Button<Msg>>,
    pub(super) samples: Handle<ComboBox<Msg>>,
    /// The code of the examples on the guide's page, for its links to insert.
    examples: RefCell<Vec<String>>,
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

/// The guide's page. A click on one of its links asks for an example, and
/// nothing else a page can report does anything but copy what is selected.
fn guide_view(ui: &Ui<Msg>) -> XuiResult<BlitzView<Msg>> {
    let example =
        |href: &str| guide_html::example_of(href).map(|i| script_msg(ScriptMsg::Example(i)));
    BlitzView::builder(
        || script_msg(ScriptMsg::GuideFrame),
        move |event| match event {
            // Our links have a scheme of their own, which the view may report as
            // one it cannot open. Either way only `example_of` can act on one.
            BlitzViewEvent::LinkClicked(href) => example(&href),
            BlitzViewEvent::LaunchUrl { url, .. } => example(&url),
            BlitzViewEvent::CopyRequested(text) => Some(script_msg(ScriptMsg::Copy(text))),
            _ => None,
        },
    )
    .build(ui, Rect::default())
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
        row().gap(6).padding(4).children((
            column().children((topics.fill(1),)).width(190),
            build(guide_view).bind(&self.guide).fill(1),
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
        let ui = self.ui.borrow();
        let (Some(ui), Some(page)) = (ui.as_ref(), guide::pages().get(topic).cloned()) else {
            return;
        };
        let theme = ui.theme();
        let made = guide_html::render(&page, &theme, guide_html::Examples::Script);
        let view = self.guide.get();
        view.set_background(guide_html::background(&theme));
        view.load_html(made.html, "about:blank");
        *self.examples.borrow_mut() = made.examples;
    }

    /// The code of the page's `n`th example.
    pub(super) fn example(&self, n: usize) -> Option<String> {
        self.examples.borrow().get(n).cloned()
    }
}
