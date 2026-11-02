//! The guide of the script pane: pages laid out by their own thread and drawn,
//! examples put in the script by a click on their link, and the guide in the
//! pane's own window.

use std::time::{Duration, Instant};

use xui::xui_canvas::snapshot::Stage;

use super::{Run, pump};
use crate::ui::script_pane::ScriptMsg;
use crate::ui::{Msg, Probe, cmd};

/// Paints until the guide has laid its page out, and once more to show it. The
/// view hands its page to its thread when painted and picks the frame up when
/// painted again, and the headless backend paints only for a picture and has no
/// wake for the thread to ask for one, so both are done here.
pub(super) fn wait_for_guide(stage: &Stage<'_, Msg>, probe: &Probe) -> bool {
    let end = Instant::now() + Duration::from_secs(10);
    while !probe.guide_ready() {
        if Instant::now() > end {
            return false;
        }
        let _ = stage.ui().capture();
        stage.emit(Msg::Script(ScriptMsg::GuideFrame));
        pump(stage, 1);
    }
    stage.emit(Msg::Script(ScriptMsg::GuideFrame));
    pump(stage, 2);
    true
}

pub(super) fn guide(run: &mut Run, stage: &Stage<'_, Msg>, probe: &Probe) {
    stage.emit(Msg::Menu(cmd::GUIDE));
    run.check(
        wait_for_guide(stage, probe),
        "the guide's first page is laid out",
    );
    run.shot(stage, "07-guide.png");
    stage.emit(Msg::Script(ScriptMsg::Topic(3)));
    run.check(
        wait_for_guide(stage, probe),
        "another page is laid out in its turn",
    );
    run.shot(stage, "08-guide-doors.png");
    stage.emit(Msg::Script(ScriptMsg::Topic(7)));
    wait_for_guide(stage, probe);
    run.shot(stage, "08-guide-reference.png");
    stage.emit(Msg::Script(ScriptMsg::Topic(3)));
    wait_for_guide(stage, probe);
    probe.code().goto(0, 2);
    let before = probe.code().text();
    // Text in the page is not a link, and the number of an example that is not
    // there asks for nothing.
    stage.click(700, 555);
    stage.emit(Msg::Script(ScriptMsg::Example(99)));
    pump(stage, 2);
    run.check(
        probe.code().text() == before,
        "a click on the page's text, or an example it does not have, changes nothing",
    );
    // The link under the first example, where the screenshot shows it.
    stage.click(250, 677);
    pump(stage, 2);
    run.check(
        probe.code().text().starts_with("gate := mover: 'gate'.\n"),
        "an example goes above the caret's line, not inside it",
    );
    run.check(
        probe.code().text().contains("gate pose: 'open'"),
        "a guide example goes in the script",
    );
    run.check(
        probe
            .state()
            .borrow()
            .script
            .text()
            .contains("gate pose: 'open'"),
        "and the model knows",
    );
    stage.emit(Msg::Menu(cmd::SCRIPT_PANE));
    pump(stage, 2);
    run.check(!probe.script_open(), "F6's command hides the pane");
    run.shot(stage, "09-closed.png");
}

/// The guide in the pane's own window: a view of its own, laid out on its own
/// thread, drawn by the window's picture.
pub(super) fn in_window(run: &mut Run, stage: &Stage<'_, Msg>, probe: &Probe) {
    stage.emit(Msg::Menu(cmd::GUIDE));
    let mut window_picture = None;
    let end = Instant::now() + Duration::from_secs(10);
    while !probe.guide_ready() && Instant::now() < end {
        window_picture = probe.script_window().and_then(|w| w.capture().ok());
        stage.emit(Msg::Script(ScriptMsg::GuideFrame));
        pump(stage, 1);
    }
    stage.emit(Msg::Script(ScriptMsg::GuideFrame));
    run.check(probe.guide_ready(), "the guide in the window is laid out");
    let shot = probe
        .script_window()
        .and_then(|w| w.capture().ok())
        .or(window_picture)
        .map(|img| img.save_png(run.dir.join("10b-own-window-guide.png")));
    run.check(
        matches!(shot, Some(Ok(()))),
        "snapshot of the guide's window",
    );
}
