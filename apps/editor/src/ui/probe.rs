//! Handles a scripted run (the self-test, unit tests) uses to read and drive
//! the editor from outside.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use super::app::{Editor, Msg};
use super::script_pane::ProbeHandles;
use super::view3d::View3d;
use crate::materials::SharedMaterials;
use crate::model::EditorState;
use crate::studio::Preview;
use xui::app::WindowHandle;
use xui_code_editor::Editor as CodeEditor;

/// What a scripted run reads and drives from outside the editor.
pub struct Probe {
    pub(super) state: Rc<RefCell<EditorState>>,
    pub(super) view3d: Rc<View3d<Msg>>,
    pub(super) play: Rc<View3d<Msg>>,
    pub(super) preview: Rc<Cell<Preview>>,
    pub(super) materials: SharedMaterials,
    pub(super) script: ProbeHandles,
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
            script: self.script.probe_handles(),
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
        self.script.code()
    }

    /// The first row of the problem list.
    pub fn problem_row(&self) -> String {
        self.script.problems().cell_text(0, 0).unwrap_or_default()
    }

    /// Whether the guide's page on show has been laid out and is drawn.
    pub fn guide_ready(&self) -> bool {
        self.script.guide_ready()
    }

    /// Whether the script pane is on show.
    pub fn script_open(&self) -> bool {
        self.script.is_open()
    }

    /// The script pane's own window, if it is up.
    pub fn script_window(&self) -> Option<WindowHandle<Msg>> {
        self.script.window()
    }

    /// The editor's 3D pane.
    pub fn view_pane(&self) -> Rc<View3d<Msg>> {
        self.view3d.clone()
    }
}
