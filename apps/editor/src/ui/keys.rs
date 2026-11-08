//! The tool palette's contents and the window-wide keyboard shortcuts.

use mscent_world::Op;
use xui::icon::Lucide;
use xui::widget::MenuId;
use xui::{Key, Modifiers};

use super::app::{Msg, cmd};
use super::icons::Glyph;
use crate::model::{EntityKind, Tool};

/// The palette: tooltip name, shortcut key, icon and tool.
pub(super) const TOOLS: [(&str, Key, Glyph, Tool); 14] = [
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
        "Projector: a light that shines one way. Set its aim and cone in the panel",
        Key::J,
        Glyph::Spot,
        Tool::Place(EntityKind::Projector),
    ),
    (
        "Enemy",
        Key::E,
        Glyph::Lucide(Lucide::Bug),
        Tool::Place(EntityKind::Enemy),
    ),
    ("Goal", Key::G, Glyph::Flag, Tool::Place(EntityKind::Goal)),
    (
        "Face / texture",
        Key::T,
        Glyph::Lucide(Lucide::Image),
        Tool::Face,
    ),
    (
        "Slice brush (Enter cuts, K picks the side)",
        Key::C,
        Glyph::Lucide(Lucide::Scissors),
        Tool::Slice,
    ),
    (
        "Move corners",
        Key::V,
        Glyph::Lucide(Lucide::CircleDot),
        Tool::Vertex,
    ),
    (
        "Marker: a named point for scripts",
        Key::M,
        Glyph::Lucide(Lucide::Star),
        Tool::Place(EntityKind::Marker),
    ),
    (
        "Zone: a box scripts are told about",
        Key::Z,
        Glyph::Lucide(Lucide::Square),
        Tool::Place(EntityKind::Zone),
    ),
    (
        "Voxel: sculpt in the 3D view. Wheel sets the radius, Ctrl swaps Add and Carve,          Alt lets one stroke build up",
        Key::X,
        Glyph::Lucide(Lucide::CircleX),
        Tool::Voxel,
    ),
    (
        "Vista layer: the eye of a view, placed in a sealed room",
        Key::Y,
        Glyph::Lucide(Lucide::Disc),
        Tool::Place(EntityKind::VistaLayer),
    ),
];

/// The new-brush operation buttons: tooltip, icon and operation, in the order
/// of the panel's Operation list.
pub(super) const OPS: [(&str, Glyph, Op); 2] = [
    (
        "New brush is solid (add)",
        Glyph::Lucide(Lucide::Plus),
        Op::Add,
    ),
    (
        "New brush carves (subtract)",
        Glyph::Lucide(Lucide::Minus),
        Op::Subtract,
    ),
];

/// A key's name for a tooltip.
pub(super) fn key_name(key: Key) -> String {
    char::from_u32(u32::from(key.code())).map_or_else(String::new, String::from)
}

/// A menu command's key. `guarded` keys are also text, or keys a text field
/// handles itself, so they are wrapped in [`Msg::Guarded`].
pub(super) struct Binding {
    pub id: MenuId,
    pub key: Key,
    pub ctrl: bool,
    pub shift: bool,
    pub guarded: bool,
}

const fn bind(id: MenuId, key: Key, ctrl: bool, shift: bool, guarded: bool) -> Binding {
    Binding {
        id,
        key,
        ctrl,
        shift,
        guarded,
    }
}

/// Every menu command's keys. The menus show them from here, so a menu label
/// and the key that does it cannot disagree. F5 needs xui 0b1b7ef or later,
/// whose winit backend reports F5 to F12; Ctrl+R is the same command.
pub(super) const BINDINGS: [Binding; 29] = [
    bind(cmd::NEW, Key::N, true, false, false),
    bind(cmd::OPEN, Key::O, true, false, false),
    bind(cmd::SAVE, Key::S, true, false, false),
    bind(cmd::PLAY, Key::F5, false, false, false),
    bind(cmd::PLAY, Key::R, true, false, false),
    bind(cmd::UNDO, Key::Z, true, false, true),
    bind(cmd::REDO, Key::Y, true, false, true),
    bind(cmd::REDO, Key::Z, true, true, true),
    bind(cmd::DELETE, Key::DELETE, false, false, true),
    bind(cmd::FLIP, Key::I, true, false, true),
    bind(cmd::MAKE_MOVER, Key::M, true, false, true),
    bind(cmd::FORWARD, Key::PAGE_DOWN, true, false, true),
    bind(cmd::BACK, Key::PAGE_UP, true, false, true),
    bind(cmd::SLICE, Key::RETURN, false, false, true),
    bind(cmd::SLICE_SIDE, Key::K, false, false, true),
    bind(cmd::CARVE, Key::C, true, true, true),
    bind(cmd::EXTRUDE, Key::E, true, false, true),
    bind(cmd::EXTRUDE_IN, Key::E, true, true, true),
    bind(cmd::LEVEL_SETTINGS, Key::F8, false, false, false),
    bind(cmd::FIT, Key::HOME, false, false, true),
    bind(cmd::RESET_CAMERA, Key::HOME, true, false, true),
    bind(cmd::LEVEL_CAMERA, Key::L, true, false, true),
    bind(cmd::GRID_FINER, Key::PAGE_DOWN, false, false, true),
    bind(cmd::GRID_COARSER, Key::PAGE_UP, false, false, true),
    bind(cmd::LAYOUT_CLASSIC, Key::DIGIT1, true, false, false),
    bind(cmd::LAYOUT_QUAD, Key::DIGIT2, true, false, false),
    bind(cmd::SCRIPT_PANE, Key::F6, false, false, false),
    bind(cmd::GUIDE, Key::F7, false, false, false),
    bind(cmd::MANUAL, Key::F1, false, false, false),
];

/// A key as a menu writes it.
fn key_text(key: Key) -> String {
    match key {
        Key::DELETE => "Del".into(),
        Key::RETURN => "Enter".into(),
        Key::PAGE_UP => "PgUp".into(),
        Key::PAGE_DOWN => "PgDn".into(),
        Key::HOME => "Home".into(),
        k if (Key::F1.code()..=Key::F12.code()).contains(&k.code()) => {
            format!("F{}", k.code() - Key::F1.code() + 1)
        }
        k => key_name(k),
    }
}

/// `text` with the keys of command `id` after it, as the menus show it:
/// "&Redo   Ctrl+Y, Ctrl+Shift+Z".
pub(super) fn menu_label(text: &str, id: MenuId) -> String {
    let keys: Vec<String> = BINDINGS
        .iter()
        .filter(|b| b.id == id)
        .map(|b| {
            let ctrl = if b.ctrl { "Ctrl+" } else { "" };
            let shift = if b.shift { "Shift+" } else { "" };
            format!("{ctrl}{shift}{}", key_text(b.key))
        })
        .collect();
    if keys.is_empty() {
        text.to_string()
    } else {
        format!("{text}   {}", keys.join(", "))
    }
}

/// Global shortcuts: they work wherever the focus is (xui offers every
/// `KeyDown` to this hook before the focused widget). Tool letters are text,
/// so they are guarded too.
pub(super) fn shortcut(key: Key, mods: Modifiers) -> Option<Msg> {
    if mods.alt || mods.win {
        return None;
    }
    let guarded = |m: Msg| Some(Msg::Guarded(Box::new(m)));
    let bound = BINDINGS
        .iter()
        .find(|b| b.key == key && b.ctrl == mods.ctrl && b.shift == mods.shift);
    if let Some(b) = bound {
        let msg = Msg::Menu(b.id);
        return if b.guarded { guarded(msg) } else { Some(msg) };
    }
    match key {
        _ if mods.ctrl => None,
        Key::ESCAPE => Some(Msg::Escape),
        _ => TOOLS
            .iter()
            .position(|t| t.1 == key && !mods.shift)
            .and_then(|i| guarded(Msg::Tool(i))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_two_bindings_share_a_key() {
        for (i, a) in BINDINGS.iter().enumerate() {
            for b in &BINDINGS[i + 1..] {
                let same = a.key == b.key && a.ctrl == b.ctrl && a.shift == b.shift;
                assert!(!same, "{:?} is bound twice", a.key);
            }
        }
    }

    #[test]
    fn menus_show_every_key_of_a_command_and_the_key_does_it() {
        assert_eq!(
            menu_label("&Redo", cmd::REDO),
            "&Redo   Ctrl+Y, Ctrl+Shift+Z"
        );
        assert_eq!(menu_label("&Play", cmd::PLAY), "&Play   F5, Ctrl+R");
        assert_eq!(
            menu_label("Level &settings...", cmd::LEVEL_SETTINGS),
            "Level &settings...   F8"
        );
        assert_eq!(
            menu_label("Open &showcase level", cmd::SHOWCASE),
            "Open &showcase level"
        );
        for b in &BINDINGS {
            let mods = Modifiers {
                ctrl: b.ctrl,
                shift: b.shift,
                ..Modifiers::NONE
            };
            let got = match shortcut(b.key, mods) {
                Some(Msg::Guarded(inner)) => *inner,
                Some(other) => other,
                None => panic!("{:?} does nothing", b.key),
            };
            assert!(matches!(got, Msg::Menu(id) if id == b.id), "{:?}", b.key);
        }
    }

    #[test]
    fn tool_letters_and_escape_still_reach_the_editor() {
        assert!(matches!(
            shortcut(Key::B, Modifiers::NONE),
            Some(Msg::Guarded(_))
        ));
        assert!(matches!(
            shortcut(Key::ESCAPE, Modifiers::NONE),
            Some(Msg::Escape)
        ));
        let ctrl = Modifiers {
            ctrl: true,
            ..Modifiers::NONE
        };
        assert!(shortcut(Key::B, ctrl).is_none());
    }
}
