#![forbid(unsafe_code)]

//! The OS text clipboard for the winit backend, over `arboard`.
//!
//! Only text is exchanged. A failure to open the clipboard (no display, a
//! locked clipboard) reads as "no text" or a silent no-op, so a copy or paste
//! never panics an app on a headless or busy session.

use arboard::Clipboard;

/// The clipboard's text, or `None` when it holds none or cannot be opened.
pub(crate) fn get() -> Option<String> {
    Clipboard::new().ok()?.get_text().ok()
}

/// Replaces the clipboard's text. Does nothing when it cannot be opened.
pub(crate) fn set(text: &str) {
    if let Ok(mut clipboard) = Clipboard::new() {
        let _ = clipboard.set_text(text.to_string());
    }
}
