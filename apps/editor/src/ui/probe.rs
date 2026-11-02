//! Handles a scripted run (the self-test, unit tests) uses to read and drive
//! the editor from outside.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use super::app::{Editor, Msg};
use super::level_dialog::FogWindow;
use super::script_pane::ProbeHandles;
use super::view3d::View3d;
use crate::materials::SharedMaterials;
use crate::model::EditorState;
use crate::studio::Preview;
use crate::texstudio::TexStudio;
use xui::app::WindowHandle;
use xui_code_editor::Editor as CodeEditor;

/// What a scripted run reads and drives from outside the editor.
pub struct Probe {
    pub(super) state: Rc<RefCell<EditorState>>,
    pub(super) view3d: Rc<View3d<Msg>>,
    pub(super) play: Rc<View3d<Msg>>,
    pub(super) preview: Rc<Cell<Preview>>,
    pub(super) materials: SharedMaterials,
    pub(super) light_key: Rc<Cell<u64>>,
    pub(super) tex: Rc<RefCell<TexStudio>>,
    pub(super) tex_preview: Rc<super::texstudio::TexPreview>,
    pub(super) script: ProbeHandles,
    pub(super) fog_window: Rc<RefCell<Option<FogWindow>>>,
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
            light_key: self.light_key.clone(),
            tex: self.tex.studio.clone(),
            tex_preview: self.tex.preview.get(),
            script: self.script.probe_handles(),
            fog_window: self.fog_window.clone(),
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

    /// What identifies the full lightmap in the picture, as of the last tick.
    pub fn light_key(&self) -> u64 {
        self.light_key.get()
    }

    /// The Texture Studio's model.
    pub fn tex(&self) -> Rc<RefCell<TexStudio>> {
        self.tex.clone()
    }

    /// The pixels on the Texture Studio's preview (width, height, RGBA).
    pub fn tex_pixels(&self) -> Option<(u32, u32, Vec<u8>)> {
        self.tex_preview.pixels()
    }

    /// The window's pixel at `u`, `v` tiles from the corner of the picture on the
    /// Texture Studio's preview, for a scripted pointer to go to.
    pub fn tex_pixel_at(&self, u: f32, v: f32) -> (i32, i32) {
        self.tex_preview.pixel_at(u, v)
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

    /// A picture of the level settings window, if it is up.
    pub fn fog_window_picture(&self) -> Option<xui::backend::Result<xui::Image>> {
        let window = self.fog_window.borrow();
        window.as_ref().filter(|w| w.is_open()).map(|w| w.capture())
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
