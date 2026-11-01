//! Handles a scripted run (the self-test, unit tests) uses to read and drive
//! the editor from outside.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use super::app::Msg;
use super::view3d::View3d;
use crate::model::EditorState;
use crate::studio::Preview;

/// What a scripted run reads and drives from outside the editor.
pub struct Probe {
    pub(super) state: Rc<RefCell<EditorState>>,
    pub(super) view3d: Rc<View3d<Msg>>,
    pub(super) play: Rc<View3d<Msg>>,
    pub(super) preview: Rc<Cell<Preview>>,
}

impl Probe {
    /// The model, shared with the views.
    pub fn state(&self) -> Rc<RefCell<EditorState>> {
        self.state.clone()
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
    /// The editor's 3D pane.
    pub fn view_pane(&self) -> Rc<View3d<Msg>> {
        self.view3d.clone()
    }
}
