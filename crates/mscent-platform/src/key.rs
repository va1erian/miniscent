//! Engine-neutral keys, mouse buttons and input events.

/// A physical key, named independently of any windowing library. Letters and
/// digits are `Char` holding the upper-case character, so a LazyOS backend
/// only has to map scancodes to these names.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Key {
    /// `A`-`Z` or `0`-`9`, always upper case.
    Char(char),
    Escape,
    Enter,
    Space,
    Tab,
    Backspace,
    Delete,
    Shift,
    Ctrl,
    Alt,
    Up,
    Down,
    Left,
    Right,
    /// Function key `F1`-`F12`.
    F(u8),
}

impl Key {
    /// Parses a key name as used in input scripts: `W`, `5`, `Space`,
    /// `Esc`, `Up`, `F3` ... Case-insensitive. `None` when unknown.
    pub fn from_name(name: &str) -> Option<Key> {
        let lower = name.to_ascii_lowercase();
        let named = match lower.as_str() {
            "esc" | "escape" => Some(Key::Escape),
            "enter" | "return" => Some(Key::Enter),
            "space" => Some(Key::Space),
            "tab" => Some(Key::Tab),
            "backspace" => Some(Key::Backspace),
            "delete" | "del" => Some(Key::Delete),
            "shift" => Some(Key::Shift),
            "ctrl" | "control" => Some(Key::Ctrl),
            "alt" => Some(Key::Alt),
            "up" => Some(Key::Up),
            "down" => Some(Key::Down),
            "left" => Some(Key::Left),
            "right" => Some(Key::Right),
            _ => None,
        };
        if named.is_some() {
            return named;
        }
        let mut chars = lower.chars();
        let first = chars.next()?;
        if chars.as_str().is_empty() && first.is_ascii_alphanumeric() {
            return Some(Key::Char(first.to_ascii_uppercase()));
        }
        let n: u8 = lower.strip_prefix('f')?.parse().ok()?;
        (1..=12).contains(&n).then_some(Key::F(n))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
}

/// One input or window event, delivered by `Platform::poll_events`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Event {
    /// A key went down (`pressed`) or up. Auto-repeat is not reported.
    Key {
        code: Key,
        pressed: bool,
    },
    /// Relative mouse movement in device units since the last event. Reported
    /// whether or not the cursor is captured.
    MouseMotion {
        dx: f32,
        dy: f32,
    },
    MouseButton {
        button: MouseButton,
        pressed: bool,
    },
    /// The drawable area changed to this size in pixels.
    Resize {
        width: u32,
        height: u32,
    },
    /// The window lost focus: hosts should drop held keys and buttons.
    FocusLost,
    /// The user asked to close the window.
    Quit,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_parse_case_insensitively() {
        assert_eq!(Key::from_name("w"), Some(Key::Char('W')));
        assert_eq!(Key::from_name("SPACE"), Some(Key::Space));
        assert_eq!(Key::from_name("Esc"), Some(Key::Escape));
        assert_eq!(Key::from_name("f3"), Some(Key::F(3)));
        assert_eq!(Key::from_name("7"), Some(Key::Char('7')));
        assert_eq!(Key::from_name("f13"), None);
        assert_eq!(Key::from_name("ww"), None);
        assert_eq!(Key::from_name(""), None);
    }
}
