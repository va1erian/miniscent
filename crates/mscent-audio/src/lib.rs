//! Sound for the game: reading `.wav` files, mixing them, and (with the
//! `device` feature) playing them.
//!
//! Sound is presentation only. Nothing here is read by the simulation, a
//! collision, a script or a replay, so a build or a machine without audio plays
//! the same game, silently (see `docs/audio.md`).

pub mod bank;
pub mod mixer;
pub mod wav;

#[cfg(feature = "device")]
pub mod device;

pub use bank::SoundBank;
pub use mixer::{Mixer, Play};
pub use wav::{Sound, WavError};
