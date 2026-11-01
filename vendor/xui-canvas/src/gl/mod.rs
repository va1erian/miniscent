#![forbid(unsafe_code)]

//! GPU rendering for portable widgets.
//!
//! A window-level [`GlWidget`] draws its frames through a [`GlSurface`] bound to
//! the window; the frame is rendered into a texture and composited through the
//! software painter model. The context and loader's `unsafe` lives in
//! [`crate::sys::gl`]; this is the safe view a widget uses.

mod renderer;
mod surface;
#[cfg(test)]
mod tests;
mod widget;

pub(crate) use renderer::RendererState;
pub use surface::{GlError, GlSurface};
pub use widget::GlWidget;
