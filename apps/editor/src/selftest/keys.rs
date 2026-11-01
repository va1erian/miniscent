//! The shortcut part of the self-test: real `KeyDown` events for F5 and Esc,
//! injected into the running editor as the window system delivers them.

use xui::backend::Event;
use xui::xui_canvas::snapshot::Stage;
use xui::{Key, Modifiers};

use super::{Run, pump, wait_for_preview};
use crate::studio::Preview;
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
    let p = wait_for_preview(stage, probe);
    run.check(
        p == Preview::Full,
        &format!("the picture is current: {p:?}"),
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
