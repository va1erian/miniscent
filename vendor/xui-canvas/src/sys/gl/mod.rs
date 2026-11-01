//! The OpenGL boundary: a `glutin` context and window surface on a `winit`
//! window, and the loader `glow` resolves its entry points through. All
//! OpenGL `unsafe` lives here.

mod context;

pub(crate) use context::Context;
