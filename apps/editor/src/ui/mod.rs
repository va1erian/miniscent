//! The xui shell around the editor model.

mod app;
mod commands;
mod paint2d;
pub mod props;
mod view3d;
mod viewport;
mod wrap;

mod probe;

pub use app::{Editor, Msg, cmd};
pub use probe::Probe;
