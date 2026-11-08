//! What the editor's widgets and shortcuts tell it.

use std::path::PathBuf;

use xui::widget::{DialogAction, MenuId};

use super::texstudio::TexMsg;
use super::viewport::ViewEvent;

#[derive(Clone)]
pub enum Msg {
    Menu(MenuId),
    /// A tool palette button, by index into the keys module's `TOOLS`.
    Tool(usize),
    NewOp(usize),
    Grid(usize),
    View(ViewEvent),
    Prop(super::props::Prop),
    Face(super::facepanel::FaceMsg),
    Script(super::script_pane::ScriptMsg),
    /// Something in the Texture Studio.
    Tex(TexMsg),
    /// The window's close button.
    CloseRequested,
    /// The level settings window's answer: the fog's start and end.
    SetFog(f32, f32),
    /// The answer to whether the script's unsaved edits may go.
    Discard(DialogAction),
    /// The frame timer.
    Tick,
    /// One UI tick on demand (scripted runs have no timers).
    Step,
    /// The Escape key anywhere: leaves Play.
    Escape,
    /// A shortcut that would also type into a text field or fight Play's own
    /// keys: dropped when one has the focus or the level is being played.
    Guarded(Box<Msg>),
    OpenPath(PathBuf),
    SavePath(PathBuf),
    /// Puts the free-fly camera somewhere (scripted runs frame a face).
    Camera(Box<crate::model::FlyCam>),
    /// A step of `--live-timing`'s script, from its pacing thread.
    Live,
}
