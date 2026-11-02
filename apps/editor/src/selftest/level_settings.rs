//! Selftest of the level settings window. The headless backend does not run a
//! second window's own messages, so its picture is checked here, and its OK comes
//! as the message it sends the editor.

use xui::xui_canvas::snapshot::Stage;

use super::{Run, pump};
use crate::ui::{Msg, Probe, cmd};

pub(super) fn script(run: &mut Run, stage: &Stage<'_, Msg>, probe: &Probe) {
    stage.emit(Msg::Menu(cmd::LEVEL_SETTINGS));
    pump(stage, 2);
    let shot = probe
        .fog_window_picture()
        .map(|img| img.map(|img| img.save_png(run.dir.join("00-level-settings.png"))));
    run.check(
        matches!(shot, Some(Ok(Ok(())))),
        "Level settings shows a window",
    );

    let depth = probe.state().borrow().history.depth();
    stage.emit(Msg::SetFog(3.0, 50.0));
    pump(stage, 1);
    let state = probe.state();
    run.check(
        state.borrow().fog() == Some((3.0, 50.0)),
        "its answer becomes the level's fog",
    );
    run.check(
        state.borrow().history.depth() == depth + 1,
        "as one undo step",
    );
    stage.emit(Msg::Menu(cmd::UNDO));
    pump(stage, 1);
    run.check(
        state.borrow().fog() != Some((3.0, 50.0)),
        "and undo takes it back",
    );

    stage.emit(Msg::Menu(cmd::LEVEL_SETTINGS));
    stage.emit(Msg::Menu(cmd::LEVEL_SETTINGS));
    pump(stage, 1);
    run.check(
        probe.fog_window_picture().is_some(),
        "asking again leaves one window up",
    );
    stage.emit(Msg::Menu(cmd::NEW));
    pump(stage, 1);
    run.check(
        probe.fog_window_picture().is_none(),
        "another level closes it: it was about the last one",
    );
}
