//! The xui shell around the editor model.

mod app;
mod commands;
pub mod facepanel;
mod faces;
mod guide_html;
mod icons;
mod keys;
pub(crate) use keys::palette_index;
mod layouts;
mod level_dialog;
mod log;
mod manual_window;
mod material_list;
mod messages;
mod paint2d;
mod playing;
pub mod props;
mod script_actions;
mod script_lexer;
pub mod script_pane;
mod script_widgets;
pub mod texstudio;
mod toolbar;
mod view3d;
mod viewport;
mod voxelpanel;
mod wrap;

mod probe;
mod quiet;

pub use app::{Editor, Msg, cmd};
pub use manual_window::ManualMsg;
pub use probe::Probe;
pub use props::Prop;
pub use view3d::{Click, PaneDrag};
pub use viewport::ViewEvent;
pub use voxelpanel::VoxelProp;
