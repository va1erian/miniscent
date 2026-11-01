//! The editor's pure model: no UI types, fully testable.

pub mod command;
pub mod document;
pub mod entity;
pub mod facetex;
pub mod fly;
pub mod pick;
pub mod pointer;
pub mod state;
pub mod view;

#[cfg(test)]
mod facetex_tests;
#[cfg(test)]
mod hostile_tests;
#[cfg(test)]
mod review_tests;
#[cfg(test)]
mod tests;

pub use document::{Document, Shape};
pub use entity::EntityKind;
pub use fly::FlyCam;
pub use pointer::Pointer;
pub use state::{EditorState, Selection, Tool};
pub use view::{Camera2D, View};
