//! The tool palette's contents and the window-wide keyboard shortcuts.

use xui::icon::Lucide;
use xui::{Key, Modifiers};

use super::app::{Msg, cmd};
use super::icons::Glyph;
use crate::model::{EntityKind, Tool};

/// The palette: tooltip name, shortcut key, icon and tool.
pub(super) const TOOLS: [(&str, Key, Glyph, Tool); 6] = [
    (
        "Select / move",
        Key::Q,
        Glyph::Lucide(Lucide::MousePointer2),
        Tool::Select,
    ),
    ("Box brush", Key::B, Glyph::Lucide(Lucide::Box), Tool::Box),
    (
        "Player start",
        Key::P,
        Glyph::Person,
        Tool::Place(EntityKind::PlayerStart),
    ),
    ("Light", Key::L, Glyph::Bulb, Tool::Place(EntityKind::Light)),
    (
        "Enemy",
        Key::E,
        Glyph::Lucide(Lucide::Bug),
        Tool::Place(EntityKind::Enemy),
    ),
    ("Goal", Key::G, Glyph::Flag, Tool::Place(EntityKind::Goal)),
];

/// The new-brush operation buttons: tooltip and icon (index 0 is Subtract).
pub(super) const OPS: [(&str, Glyph); 2] = [
    ("New brush carves (subtract)", Glyph::Lucide(Lucide::Minus)),
    ("New brush is solid (add)", Glyph::Lucide(Lucide::Plus)),
];

/// A key's name for a tooltip.
pub(super) fn key_name(key: Key) -> String {
    char::from_u32(u32::from(key.code())).map_or_else(String::new, String::from)
}

/// Global shortcuts: they work wherever the focus is (xui offers every
/// `KeyDown` to this hook before the focused widget). Keys that are also
/// text (tool letters) or that a text field handles itself (Delete, Ctrl+Z)
/// are wrapped in [`Msg::Guarded`].
///
/// xui's winit backend only reports F1 to F4, so F5 and F7 never arrive in a
/// real window (see `docs/xui-patches/NOTES.md`); Ctrl+R and Ctrl+B are the
/// same commands on keys that do arrive.
pub(super) fn shortcut(key: Key, mods: Modifiers) -> Option<Msg> {
    let guarded = |m: Msg| Some(Msg::Guarded(Box::new(m)));
    if mods.ctrl {
        return match key {
            Key::S => Some(Msg::Menu(cmd::SAVE)),
            Key::O => Some(Msg::Menu(cmd::OPEN)),
            Key::N => Some(Msg::Menu(cmd::NEW)),
            Key::B => Some(Msg::Menu(cmd::BUILD)),
            Key::R => Some(Msg::Menu(cmd::PLAY)),
            Key::Z if !mods.shift => guarded(Msg::Menu(cmd::UNDO)),
            Key::Y => guarded(Msg::Menu(cmd::REDO)),
            Key::Z => guarded(Msg::Menu(cmd::REDO)),
            _ => None,
        };
    }
    if mods.alt || mods.win {
        return None;
    }
    match key {
        Key::F5 => Some(Msg::Menu(cmd::PLAY)),
        Key::F7 => Some(Msg::Menu(cmd::BUILD)),
        Key::ESCAPE => Some(Msg::Escape),
        Key::DELETE => guarded(Msg::Menu(cmd::DELETE)),
        _ => TOOLS
            .iter()
            .position(|t| t.1 == key && !mods.shift)
            .and_then(|i| guarded(Msg::Tool(i))),
    }
}
