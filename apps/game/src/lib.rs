//! The miniscent game runtime, as a library.
//!
//! * [`GameSession`] (`session.rs`): a playable level without a window.
//!   `step(dt, events)` advances the 120 Hz simulation and menus from
//!   platform events; `render(w, h)` returns the frame. The editor embeds it
//!   for Instant Play by translating its own input into
//!   `mscent_platform::Event`s.
//! * [`app::run`]: the main loop over any `mscent_platform::Platform`, with
//!   the adaptive internal resolution.
//! * [`cli`], [`assets`], [`levels`], [`lightcache`]: command line, file
//!   lookup, built-in levels, where baked lightmaps are kept.
//!
//! The game depends on `mscent-platform` only; no GUI toolkit. The `desktop`
//! feature (on by default for the binary) adds the winit window backend.

pub mod app;
pub mod assets;
pub mod cli;
pub mod controls;
mod feedback;
pub mod levels;
pub mod lightcache;
pub mod scripts;
pub mod session;

pub use session::{FrameStats, GameSession, SessionConfig, SessionError, StepOutcome};
