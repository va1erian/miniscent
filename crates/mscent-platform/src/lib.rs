//! The platform contract of the game: what a host must provide so the same
//! game code runs on a desktop window, in tests and, later, on LazyOS.
//!
//! The game never names a windowing library. It owns an RGBA framebuffer
//! and talks to a [`Platform`]:
//!
//! * [`Platform::present`] shows a `width * height` slice of `0xAARRGGBB`
//!   pixels (alpha ignored). The size is the game's internal resolution, not
//!   the window's; the backend scales it to the window (the desktop backend
//!   uses an integer nearest-neighbour scale with letterbox).
//! * [`Platform::poll_events`] returns the input that arrived since the last
//!   call, in order: [`Event::Key`] (physical keys, no auto-repeat),
//!   [`Event::MouseMotion`] (relative deltas), [`Event::MouseButton`],
//!   [`Event::Resize`], [`Event::FocusLost`] and [`Event::Quit`].
//! * [`Platform::now`] is a monotonic clock measured from backend start.
//! * [`Platform::set_mouse_captured`] asks for a hidden, confined cursor with
//!   relative motion; a backend without a pointer may ignore it.
//! * [`Platform::should_quit`] turns true once the window was closed or the
//!   scripted run is over.
//!
//! # Adding a LazyOS backend
//!
//! Implement [`Platform`] over the LazyOS framebuffer and input syscalls and
//! map scancodes to [`Key`]. Nothing else in the game changes. The crate is
//! `std` only by default; the winit and softbuffer dependencies exist only
//! with the `desktop` cargo feature.
//!
//! # Backends
//!
//! * [`HeadlessPlatform`] (always built): a virtual clock, a [`Script`] of
//!   timed input and captured frames, for tests and screenshots.
//! * `DesktopPlatform` (feature `desktop`): winit 0.30 and softbuffer 0.4.

use std::time::Duration;

pub mod headless;
pub mod key;
pub mod scale;
pub mod script;

#[cfg(feature = "desktop")]
pub mod desktop;

pub use headless::{Frame, HeadlessPlatform};
pub use key::{Event, Key, MouseButton};
pub use script::{Action, Script, ScriptError};

#[cfg(feature = "desktop")]
pub use desktop::{DesktopConfig, DesktopPlatform};

/// What the game needs from its host. See the crate docs for the contract.
pub trait Platform {
    /// Drawable size in pixels (the window's inner size).
    fn size(&self) -> (u32, u32);
    /// Shows a frame of `width * height` pixels, `0xAARRGGBB`. A slice whose
    /// length does not match is ignored.
    fn present(&mut self, pixels: &[u32], width: u32, height: u32);
    /// Input that arrived since the previous call. May pump the OS event loop.
    fn poll_events(&mut self) -> Vec<Event>;
    /// Monotonic time since the backend started.
    fn now(&self) -> Duration;
    /// Hides and confines the cursor and reports relative motion, or undoes it.
    fn set_mouse_captured(&mut self, captured: bool);
    /// True once the run should end.
    fn should_quit(&self) -> bool;
}
