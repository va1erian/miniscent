//! Platform boundary of the canvas backend. Every `unsafe` block in the crate
//! lives under this module: [`gl`] wraps `glutin`'s context creation and the
//! `glow` loader behind a safe API, [`double_click`] reads the system's
//! double-click thresholds and [`animation`] its animation preference.

pub(crate) mod animation;
pub(crate) mod double_click;
pub(crate) mod gl;
