//! Keyboard shortcuts through xui's offscreen backend: real `KeyDown` events
//! injected into the built editor, as the window system delivers them.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use xui::backend::Event;
use xui::xui_canvas::snapshot::{Snapshot, Stage, render_with};
use xui::{Dip, Theme};

use super::*;
use crate::model::{EntityKind, Selection, Tool};
use xui::{Key, Modifiers};

/// Presses and releases `key` with `modifiers`.
fn press(stage: &Stage<'_, Msg>, key: Key, modifiers: Modifiers) {
    stage.inject(Event::KeyDown {
        key,
        modifiers,
        repeat: 1,
        system: false,
    });
    stage.inject(Event::KeyUp {
        key,
        modifiers,
        system: false,
    });
}

/// The model and a number field of the properties panel.
type Handles = (Rc<RefCell<EditorState>>, Rc<NumberField<Msg>>);

const CTRL: Modifiers = Modifiers {
    ctrl: true,
    ..Modifiers::NONE
};

/// Steps the app until the status line satisfies `done`.
fn wait(stage: &Stage<'_, Msg>, state: &Rc<RefCell<EditorState>>, done: impl Fn(&str) -> bool) {
    for _ in 0..30_000 {
        stage.emit(Msg::Step);
        if done(&state.borrow().message) {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    panic!("timed out; status is {:?}", state.borrow().message);
}

#[test]
fn shortcuts_play_edit_and_select_tools_wherever_focus_is() {
    let probe: Rc<RefCell<Option<Handles>>> = Rc::new(RefCell::new(None));
    let p2 = probe.clone();
    let ran = Rc::new(Cell::new(false));
    let r2 = ran.clone();
    render_with(
        Snapshot::new(Dip(1280.0), Dip(800.0)).theme(Theme::light()),
        move |ui| {
            let app = Editor::build(ui, Document::from_level(&mscent_world::showcase_level()))?;
            *p2.borrow_mut() = Some((app.probe().state(), app.panel.min_x()));
            Ok(app)
        },
        move |stage| {
            let probe = probe.borrow();
            let (state, field) = probe.as_ref().expect("built");
            let tool = || state.borrow().tool;
            let brushes = || state.borrow().doc.brushes().len();

            // There is nothing to build any more: F7 and Ctrl+B do nothing.
            let before = state.borrow().message.clone();
            press(stage, Key::F7, Modifiers::NONE);
            press(stage, Key::B, CTRL);
            assert_eq!(state.borrow().message, before);

            // F5 plays as soon as the first preview is up; Esc comes back.
            press(stage, Key::F5, Modifiers::NONE);
            assert_eq!(state.borrow().message, "Preparing the level...");
            wait(stage, state, |m| m.starts_with("Playing"));
            press(stage, Key::ESCAPE, Modifiers::NONE);
            wait(stage, state, |m| m == "Back in the editor");

            // Ctrl+R is the same command on a key every backend sends.
            press(stage, Key::R, CTRL);
            wait(stage, state, |m| m.starts_with("Playing"));
            press(stage, Key::ESCAPE, Modifiers::NONE);
            wait(stage, state, |m| m == "Back in the editor");

            // Tool letters.
            for (key, want) in [
                (Key::B, Tool::Box),
                (Key::E, Tool::Place(EntityKind::Enemy)),
                (Key::Q, Tool::Select),
            ] {
                press(stage, key, Modifiers::NONE);
                assert_eq!(tool(), want);
            }

            // Delete removes the selection, Ctrl+Z and Ctrl+Y undo and redo it.
            let id = state.borrow().doc.brushes()[0].id;
            state.borrow_mut().selection = Some(Selection::Brush(id));
            let before = brushes();
            press(stage, Key::DELETE, Modifiers::NONE);
            assert_eq!(brushes(), before - 1);
            press(stage, Key::Z, CTRL);
            assert_eq!(brushes(), before);
            press(stage, Key::Y, CTRL);
            assert_eq!(brushes(), before - 1);
            press(stage, Key::Z, CTRL);

            // A number field has the focus: its keys are text, not shortcuts.
            state.borrow_mut().selection = Some(Selection::Brush(id));
            stage.emit(Msg::Step);
            field.focus();
            press(stage, Key::E, Modifiers::NONE);
            assert_eq!(tool(), Tool::Select, "E typed into the field");
            press(stage, Key::DELETE, Modifiers::NONE);
            assert_eq!(brushes(), before, "Delete edited the field");
            // Window-level shortcuts still work from the field.
            press(stage, Key::F5, Modifiers::NONE);
            wait(stage, state, |m| m.starts_with("Playing"));
            press(stage, Key::ESCAPE, Modifiers::NONE);
            ran.set(true);
        },
    )
    .expect("render");
    assert!(r2.get(), "the scripted step ran to the end");
}
