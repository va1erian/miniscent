//! The xui shell around the editor model.

mod app;
mod commands;
pub mod facepanel;
mod faces;
mod icons;
mod keys;
mod layouts;
mod level_dialog;
mod paint2d;
pub mod props;
mod toolbar;
mod view3d;
mod viewport;
mod wrap;

mod probe;

pub use app::{Editor, Msg, cmd};
pub use probe::Probe;
pub use props::Prop;
pub use view3d::{Click, PaneDrag};
