//! Handles a scripted run (the self-test, unit tests) uses to read and drive
//! the editor from outside.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use super::app::{Editor, Msg};
use super::view3d::View3d;
use crate::materials::SharedMaterials;
use crate::model::EditorState;
use crate::studio::Preview;
use xui::widget::ListView;
use xui_code_editor::Editor as CodeEditor;

/// What a scripted run reads and drives from outside the editor.
pub struct Probe {
    pub(super) state: Rc<RefCell<EditorState>>,
    pub(super) view3d: Rc<View3d<Msg>>,
    pub(super) play: Rc<View3d<Msg>>,
    pub(super) preview: Rc<Cell<Preview>>,
    pub(super) materials: SharedMaterials,
    pub(super) code: Rc<CodeEditor<Msg>>,
    pub(super) problems: Rc<ListView<Msg>>,
    pub(super) script_open: Rc<Cell<bool>>,
}

impl Editor {
    /// Handles for scripted runs and tests.
    pub fn probe(&self) -> Probe {
        Probe {
            state: self.state.clone(),
            view3d: self.pane3d(),
            play: self.play.get(),
            preview: self.preview.clone(),
            materials: self.studio.materials().clone(),
            code: self.script.code.get(),
            problems: self.script.problems(),
            script_open: self.script.open_flag(),
        }
    }
}

impl Probe {
    /// The model, shared with the views.
    pub fn state(&self) -> Rc<RefCell<EditorState>> {
        self.state.clone()
    }

    /// The material table, which a run can replace.
    pub fn materials(&self) -> &SharedMaterials {
        &self.materials
    }

    /// How far the picture had caught up with the document at the last tick.
    pub fn preview(&self) -> Preview {
        self.preview.get()
    }

    /// The last status message.
    pub fn message(&self) -> String {
        self.state.borrow().message.clone()
    }

    /// The Instant Play pane (covers the work area while playing).
    pub fn play_pane(&self) -> Rc<View3d<Msg>> {
        self.play.clone()
    }
}

impl Probe {
    /// The script pane's code editor.
    pub fn code(&self) -> Rc<CodeEditor<Msg>> {
        self.code.clone()
    }

    /// The first row of the problem list.
    pub fn problem_row(&self) -> String {
        self.problems.cell_text(0, 0).unwrap_or_default()
    }

    /// Whether the script pane is on show.
    pub fn script_open(&self) -> bool {
        self.script_open.get()
    }

    /// The editor's 3D pane.
    pub fn view_pane(&self) -> Rc<View3d<Msg>> {
        self.view3d.clone()
    }
}
