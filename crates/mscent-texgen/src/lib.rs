//! Procedural textures from small recipes: noise, drawing, filter and combine
//! operators on one pixel format, in the manner of .kkrieger's Werkkzeug. See
//! `docs/texture-studio.md`.
//!
//! Everything here is deterministic. Kernels use `+ - * /`, `sqrt` and the
//! series in [`math`], never the platform's `sin` or `pow`, so one recipe gives
//! one set of bytes. Everything tiles: see [`ops`] for how that is tested.
//! Parameters and recipes are untrusted input, so failures are [`TexError`]s.

pub mod error;
pub mod image;
pub mod math;
pub mod noise;
pub mod ops;

pub use error::TexError;
pub use image::{Frame, Image, Px, Size};
