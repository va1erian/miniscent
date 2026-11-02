//! The editor's pure model: no UI types, fully testable.

pub mod brush_def;
pub mod command;
pub mod document;
pub mod entity;
pub mod facetex;
pub mod fly;
pub mod generators;
mod growth;
pub mod labels;
pub mod outline;
pub mod pick;
pub mod pointer;
mod sculptor;
pub mod shape_ops;
pub mod state;
pub mod stroke;
pub mod texdoc;
pub mod tools;
pub mod view;
pub mod voxel;

#[cfg(test)]
mod facetex_tests;
#[cfg(test)]
mod growth_tests;
#[cfg(test)]
mod hostile_tests;
#[cfg(test)]
mod outline_tests;
#[cfg(test)]
mod review_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod tools_tests;
#[cfg(test)]
mod voxel_tests;

pub use document::{Document, Shape};
pub use entity::EntityKind;
pub use fly::FlyCam;
pub use pointer::Pointer;
pub use state::{EditorState, Selection, Tool};
pub use tools::Keep;
pub use view::{Camera2D, View};
#[cfg(test)]
mod label_tests;
