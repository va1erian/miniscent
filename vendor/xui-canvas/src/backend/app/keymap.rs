#![forbid(unsafe_code)]

//! Pure translations from `winit`'s input vocabulary to the portable one:
//! pointer shapes, mouse buttons and logical keys.

use winit::event::MouseButton as WinitButton;
use winit::keyboard::{Key as WinitKey, NamedKey};
use winit::window::{CursorIcon, ResizeDirection};

use xui_core::backend::{Cursor, Event};
use xui_core::message::{Key, Modifiers, MouseButton};

/// The portable events one `winit` key event produces.
///
/// A key with a portable code raises `KeyDown` (or `KeyUp`). A press that
/// produces text also raises one `Char` per non-control character, **whether or
/// not the key has a code**: punctuation, symbols and accented letters (`;`,
/// `{`, `é`, AltGr combinations on non-US layouts) have no [`Key`] of their own
/// but must still type.
pub(super) fn key_events(
    pressed: bool,
    repeat: bool,
    logical: &WinitKey,
    text: Option<&str>,
    modifiers: Modifiers,
) -> Vec<Event> {
    let mut events = Vec::new();
    if let Some(key) = virtual_key(logical) {
        events.push(if pressed {
            Event::KeyDown {
                key,
                modifiers,
                repeat: if repeat { 2 } else { 1 },
                system: false,
            }
        } else {
            Event::KeyUp {
                key,
                modifiers,
                system: false,
            }
        });
    }
    if pressed && types_text(modifiers) {
        events.extend(
            text.into_iter()
                .flat_map(str::chars)
                .filter(|character| !character.is_control())
                .map(Event::Char),
        );
    }
    events
}

/// Whether a press with `modifiers` types its text. `winit`'s `text` ignores
/// Ctrl, so Ctrl+C reports "c": typing it would insert the letter after the
/// shortcut runs. A shortcut modifier (Ctrl or the Windows/Command key) means
/// no text, except Ctrl+Alt, which is how Windows reports AltGr, the key that
/// types `{ [ @ #` on many non-US layouts.
fn types_text(modifiers: Modifiers) -> bool {
    // Only a bare Ctrl+Alt is AltGr; with Win/Command it is still a shortcut.
    let altgr = modifiers.ctrl && modifiers.alt && !modifiers.win;
    altgr || !(modifiers.ctrl || modifiers.win)
}

/// The frame thickness, in device pixels, within which a borderless resizable
/// window treats a pointer as being on a resize edge.
pub(super) const RESIZE_BORDER_PX: i32 = 5;

/// The `winit` icon for a portable pointer shape.
pub(super) fn cursor_icon(cursor: Cursor) -> CursorIcon {
    match cursor {
        Cursor::Default => CursorIcon::Default,
        Cursor::Hand => CursorIcon::Pointer,
        Cursor::Text => CursorIcon::Text,
        Cursor::SizeHorizontal => CursorIcon::EwResize,
        Cursor::SizeVertical => CursorIcon::NsResize,
        Cursor::SizeNwSe => CursorIcon::NwseResize,
        Cursor::SizeNeSw => CursorIcon::NeswResize,
        Cursor::Busy => CursorIcon::Wait,
    }
}

/// The resize direction for a pointer at `(x, y)` in a `width`×`height` client
/// area, when it is within `border` device pixels of an edge.
///
/// Returns `None` in the interior, when `resizable` is false, when `border` is
/// not positive, or when an axis is too short for its two edges to be distinct.
pub(super) fn resize_direction(
    x: i32,
    y: i32,
    width: u32,
    height: u32,
    border: i32,
    resizable: bool,
) -> Option<ResizeDirection> {
    if !resizable || border <= 0 {
        return None;
    }
    let (width, height) = (width as i32, height as i32);
    let horizontal = width >= 2 * border;
    let vertical = height >= 2 * border;
    let west = horizontal && x < border;
    let east = horizontal && x >= width - border;
    let north = vertical && y < border;
    let south = vertical && y >= height - border;
    Some(match (north, south, west, east) {
        (true, _, true, _) => ResizeDirection::NorthWest,
        (true, _, _, true) => ResizeDirection::NorthEast,
        (_, true, true, _) => ResizeDirection::SouthWest,
        (_, true, _, true) => ResizeDirection::SouthEast,
        (true, _, _, _) => ResizeDirection::North,
        (_, true, _, _) => ResizeDirection::South,
        (_, _, true, _) => ResizeDirection::West,
        (_, _, _, true) => ResizeDirection::East,
        _ => return None,
    })
}

/// The pointer icon shown over the frame for a resize direction. The edges use
/// the bidirectional arrows a border draws; the corners use diagonals.
pub(super) fn resize_cursor(direction: ResizeDirection) -> CursorIcon {
    match direction {
        ResizeDirection::East | ResizeDirection::West => CursorIcon::EwResize,
        ResizeDirection::North | ResizeDirection::South => CursorIcon::NsResize,
        ResizeDirection::NorthEast | ResizeDirection::SouthWest => CursorIcon::NeswResize,
        ResizeDirection::NorthWest | ResizeDirection::SouthEast => CursorIcon::NwseResize,
    }
}

pub(super) fn mouse_button(button: WinitButton) -> Option<MouseButton> {
    Some(match button {
        WinitButton::Left => MouseButton::Left,
        WinitButton::Right => MouseButton::Right,
        WinitButton::Middle => MouseButton::Middle,
        WinitButton::Back => MouseButton::X1,
        WinitButton::Forward => MouseButton::X2,
        _ => return None,
    })
}

/// Maps a `winit` logical key to the `VK_*` code the portable widgets use.
pub(super) fn virtual_key(key: &WinitKey) -> Option<Key> {
    Some(match key {
        WinitKey::Named(named) => match named {
            NamedKey::Enter => Key::RETURN,
            NamedKey::Space => Key::SPACE,
            NamedKey::Escape => Key::ESCAPE,
            NamedKey::Tab => Key::TAB,
            NamedKey::Backspace => Key::BACK,
            NamedKey::Delete => Key::DELETE,
            NamedKey::Insert => Key::INSERT,
            NamedKey::Home => Key::HOME,
            NamedKey::End => Key::END,
            NamedKey::PageUp => Key::PAGE_UP,
            NamedKey::PageDown => Key::PAGE_DOWN,
            NamedKey::ArrowLeft => Key::LEFT,
            NamedKey::ArrowRight => Key::RIGHT,
            NamedKey::ArrowUp => Key::UP,
            NamedKey::ArrowDown => Key::DOWN,
            NamedKey::F1 => Key::F1,
            NamedKey::F2 => Key::F2,
            NamedKey::F3 => Key::F3,
            NamedKey::F4 => Key::F4,
            NamedKey::F5 => Key::F5,
            NamedKey::F6 => Key::F6,
            NamedKey::F7 => Key::F7,
            NamedKey::F8 => Key::F8,
            NamedKey::F9 => Key::F9,
            NamedKey::F10 => Key::F10,
            NamedKey::F11 => Key::F11,
            NamedKey::F12 => Key::F12,
            _ => return None,
        },
        WinitKey::Character(text) => {
            let character = text.chars().next()?;
            if character.is_ascii_alphabetic() {
                Key::from_code(character.to_ascii_uppercase() as u16)
            } else if character.is_ascii_digit() {
                Key::from_code(character as u16)
            } else {
                return None;
            }
        }
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn named_keys_map_to_their_vk_codes() {
        assert_eq!(
            virtual_key(&WinitKey::Named(NamedKey::Enter)),
            Some(Key::RETURN)
        );
        assert_eq!(
            virtual_key(&WinitKey::Named(NamedKey::ArrowLeft)),
            Some(Key::LEFT)
        );
        assert_eq!(
            virtual_key(&WinitKey::Named(NamedKey::Escape)),
            Some(Key::ESCAPE)
        );
        assert_eq!(virtual_key(&WinitKey::Named(NamedKey::F1)), Some(Key::F1));
        assert_eq!(virtual_key(&WinitKey::Named(NamedKey::F5)), Some(Key::F5));
        assert_eq!(virtual_key(&WinitKey::Named(NamedKey::F7)), Some(Key::F7));
        assert_eq!(virtual_key(&WinitKey::Named(NamedKey::F12)), Some(Key::F12));
    }

    #[test]
    fn character_keys_map_case_insensitively() {
        assert_eq!(virtual_key(&WinitKey::Character("a".into())), Some(Key::A));
        assert_eq!(
            virtual_key(&WinitKey::Character("7".into())),
            Some(Key::DIGIT7)
        );
        assert_eq!(virtual_key(&WinitKey::Character("-".into())), None);
    }

    #[test]
    fn the_resize_cursors_map_to_their_winit_icons() {
        assert_eq!(cursor_icon(Cursor::Hand), CursorIcon::Pointer);
        assert_eq!(cursor_icon(Cursor::SizeHorizontal), CursorIcon::EwResize);
        assert_eq!(cursor_icon(Cursor::SizeVertical), CursorIcon::NsResize);
        assert_eq!(cursor_icon(Cursor::SizeNwSe), CursorIcon::NwseResize);
        assert_eq!(cursor_icon(Cursor::SizeNeSw), CursorIcon::NeswResize);
        assert_eq!(cursor_icon(Cursor::Busy), CursorIcon::Wait);
    }

    #[test]
    fn only_the_mouse_buttons_the_core_models_are_mapped() {
        assert_eq!(mouse_button(WinitButton::Left), Some(MouseButton::Left));
        assert_eq!(mouse_button(WinitButton::Back), Some(MouseButton::X1));
        assert_eq!(mouse_button(WinitButton::Forward), Some(MouseButton::X2));
    }

    #[test]
    fn each_edge_of_the_frame_maps_to_a_resize_direction() {
        // A 100x80 window with a 4px frame, probed at the middle of each edge.
        assert_eq!(
            resize_direction(0, 40, 100, 80, 4, true),
            Some(ResizeDirection::West)
        );
        assert_eq!(
            resize_direction(99, 40, 100, 80, 4, true),
            Some(ResizeDirection::East)
        );
        assert_eq!(
            resize_direction(50, 0, 100, 80, 4, true),
            Some(ResizeDirection::North)
        );
        assert_eq!(
            resize_direction(50, 79, 100, 80, 4, true),
            Some(ResizeDirection::South)
        );
    }

    #[test]
    fn each_corner_of_the_frame_maps_to_a_diagonal_direction() {
        assert_eq!(
            resize_direction(0, 0, 100, 80, 4, true),
            Some(ResizeDirection::NorthWest)
        );
        assert_eq!(
            resize_direction(99, 0, 100, 80, 4, true),
            Some(ResizeDirection::NorthEast)
        );
        assert_eq!(
            resize_direction(0, 79, 100, 80, 4, true),
            Some(ResizeDirection::SouthWest)
        );
        assert_eq!(
            resize_direction(99, 79, 100, 80, 4, true),
            Some(ResizeDirection::SouthEast)
        );
    }

    #[test]
    fn the_interior_is_not_a_resize_edge() {
        assert_eq!(resize_direction(50, 40, 100, 80, 4, true), None);
        assert_eq!(resize_direction(4, 40, 100, 80, 4, true), None);
        assert_eq!(resize_direction(50, 4, 100, 80, 4, true), None);
    }

    #[test]
    fn a_non_resizable_window_has_no_resize_edges() {
        assert_eq!(resize_direction(0, 0, 100, 80, 4, false), None);
        assert_eq!(resize_direction(50, 40, 100, 80, 4, false), None);
    }

    #[test]
    fn a_window_too_small_for_two_edges_has_no_resize_edges() {
        // Narrower than twice the frame, so no horizontal edge is distinct.
        assert_eq!(resize_direction(0, 40, 6, 80, 4, true), None);
        assert_eq!(
            resize_direction(50, 0, 6, 80, 4, true),
            Some(ResizeDirection::North)
        );
    }

    #[test]
    fn the_resize_cursors_follow_the_direction() {
        assert_eq!(resize_cursor(ResizeDirection::East), CursorIcon::EwResize);
        assert_eq!(resize_cursor(ResizeDirection::West), CursorIcon::EwResize);
        assert_eq!(resize_cursor(ResizeDirection::North), CursorIcon::NsResize);
        assert_eq!(resize_cursor(ResizeDirection::South), CursorIcon::NsResize);
        assert_eq!(
            resize_cursor(ResizeDirection::NorthEast),
            CursorIcon::NeswResize
        );
        assert_eq!(
            resize_cursor(ResizeDirection::NorthWest),
            CursorIcon::NwseResize
        );
    }

    #[test]
    fn punctuation_and_accents_type_although_they_have_no_key_code() {
        for text in [";", ",", ".", "{", "(", "\"", "é", "à", "ç"] {
            let events = key_events(
                true,
                false,
                &WinitKey::Character(text.into()),
                Some(text),
                Modifiers::NONE,
            );
            let typed = text.chars().next().unwrap();
            assert_eq!(events, vec![Event::Char(typed)], "{text:?} types");
        }
    }

    #[test]
    fn a_letter_raises_key_down_then_its_character() {
        let events = key_events(
            true,
            false,
            &WinitKey::Character("a".into()),
            Some("a"),
            Modifiers::NONE,
        );
        assert!(matches!(
            events[0],
            Event::KeyDown {
                key: Key::A,
                repeat: 1,
                ..
            }
        ));
        assert_eq!(events[1], Event::Char('a'));
        assert_eq!(events.len(), 2);
    }

    #[test]
    fn releases_and_control_text_type_nothing() {
        let release = key_events(
            false,
            false,
            &WinitKey::Character(";".into()),
            Some(";"),
            Modifiers::NONE,
        );
        assert!(
            release.is_empty(),
            "a released punctuation key raises nothing"
        );
        let ctrl_c = key_events(
            true,
            false,
            &WinitKey::Character("c".into()),
            Some("\u{3}"),
            Modifiers {
                ctrl: true,
                ..Modifiers::NONE
            },
        );
        assert_eq!(
            ctrl_c.len(),
            1,
            "Ctrl+C is a key, not a typed control character"
        );
    }

    fn press(text: &str, modifiers: Modifiers) -> Vec<Event> {
        key_events(
            true,
            false,
            &WinitKey::Character(text.into()),
            Some(text),
            modifiers,
        )
    }

    #[test]
    fn shortcuts_do_not_type_their_letter() {
        let ctrl = Modifiers {
            ctrl: true,
            ..Modifiers::NONE
        };
        for letter in ["c", "v", "x", "a", "z", "s"] {
            let events = press(letter, ctrl);
            assert!(
                !events.iter().any(|event| matches!(event, Event::Char(_))),
                "Ctrl+{letter} types nothing: {events:?}"
            );
            assert!(
                matches!(events[0], Event::KeyDown { .. }),
                "Ctrl+{letter} is still a key"
            );
        }
        let unmapped = press(";", ctrl);
        assert!(
            unmapped.is_empty(),
            "Ctrl+; neither types nor has a key code"
        );
        let with_win = Modifiers {
            ctrl: true,
            alt: true,
            win: true,
            ..Modifiers::NONE
        };
        assert!(
            !press("c", with_win)
                .iter()
                .any(|event| matches!(event, Event::Char(_))),
            "Ctrl+Alt+Win is a shortcut, not AltGr"
        );
        let win = press(
            "c",
            Modifiers {
                win: true,
                ..Modifiers::NONE
            },
        );
        assert!(
            !win.iter().any(|event| matches!(event, Event::Char(_))),
            "Win/Cmd+C types nothing"
        );
    }

    #[test]
    fn altgr_and_shift_still_type() {
        let altgr = Modifiers {
            ctrl: true,
            alt: true,
            ..Modifiers::NONE
        };
        assert_eq!(
            press("{", altgr),
            vec![Event::Char('{')],
            "AltGr+4 on AZERTY types {{"
        );
        let shift = Modifiers {
            shift: true,
            ..Modifiers::NONE
        };
        let events = press("A", shift);
        assert_eq!(events.last(), Some(&Event::Char('A')), "Shift+a types A");
    }
}
