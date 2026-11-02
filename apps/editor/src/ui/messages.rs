//! What the editor's widgets and shortcuts tell it, and the menu's command ids.

use std::path::PathBuf;

use xui::widget::MenuId;

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
    /// Something in the Texture Studio.
    Tex(TexMsg),
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
}

/// Menu command ids.
pub mod cmd {
    use xui::widget::MenuId;
    pub const NEW: MenuId = MenuId::new(1);
    pub const OPEN: MenuId = MenuId::new(2);
    pub const SAVE: MenuId = MenuId::new(3);
    pub const PLAY: MenuId = MenuId::new(5);
    pub const SHOWCASE: MenuId = MenuId::new(6);
    pub const SAVE_AS: MenuId = MenuId::new(7);
    pub const UNDO: MenuId = MenuId::new(10);
    pub const REDO: MenuId = MenuId::new(11);
    pub const MAKE_MOVER: MenuId = MenuId::new(26);
    pub const DELETE: MenuId = MenuId::new(12);
    pub const FLIP: MenuId = MenuId::new(13);
    pub const FORWARD: MenuId = MenuId::new(14);
    pub const BACK: MenuId = MenuId::new(15);
    pub const SLICE: MenuId = MenuId::new(16);
    pub const SLICE_SIDE: MenuId = MenuId::new(17);
    pub const CARVE: MenuId = MenuId::new(18);
    pub const EXTRUDE: MenuId = MenuId::new(19);
    pub const EXTRUDE_IN: MenuId = MenuId::new(22);
    pub const FIT: MenuId = MenuId::new(20);
    pub const RESET_CAMERA: MenuId = MenuId::new(21);
    pub const LAYOUT_CLASSIC: MenuId = MenuId::new(23);
    pub const LAYOUT_QUAD: MenuId = MenuId::new(24);
    pub const LEVEL_SETTINGS: MenuId = MenuId::new(25);
    pub const TEX_OPEN: MenuId = MenuId::new(30);
    pub const TEX_CLOSE: MenuId = MenuId::new(31);
    pub const TEX_NEW: MenuId = MenuId::new(32);
    pub const TEX_APPLY: MenuId = MenuId::new(33);
    pub const TEX_SAVE: MenuId = MenuId::new(34);
}
