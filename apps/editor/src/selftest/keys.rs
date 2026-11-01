//! The shortcut part of the self-test: real `KeyDown` events for F7 and F5,
//! injected into the running editor as the window system delivers them.

use xui::backend::Event;
use xui::xui_canvas::snapshot::Stage;
use xui::{Key, Modifiers};

use super::{Run, pump, wait_for_build};
use crate::ui::{Msg, Probe};

fn press(stage: &Stage<'_, Msg>, key: Key) {
    let modifiers = Modifiers::NONE;
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

pub(super) fn script(run: &mut Run, stage: &Stage<'_, Msg>, probe: &Probe) {
    press(stage, Key::F7);
    let m = wait_for_build(stage, probe);
    run.check(
        m.starts_with("Build finished"),
        &format!("injected F7 KeyDown built the level: {m}"),
    );
    press(stage, Key::F5);
    pump(stage, 5);
    run.check(
        probe.message().starts_with("Playing"),
        &format!("injected F5 KeyDown started play: {}", probe.message()),
    );
    press(stage, Key::ESCAPE);
    pump(stage, 5);
    run.check(
        probe.message() == "Back in the editor",
        &format!("injected Esc returned: {}", probe.message()),
    );
}
