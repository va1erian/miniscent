//! The guide of the script pane: pages drawn, examples put in the script by a
//! click on their link, and the guide in the pane's own window. And the editor
//! manual: F1 opens it on the page about the tool in hand, each page is drawn
//! in its window, and Esc's message closes it. The page views lay out what they
//! show when painted, so drawing a picture is all a page needs.

use xui::xui_canvas::snapshot::Stage;

use super::{Run, pump};
use crate::manual;
use crate::model::Tool;
use crate::ui::script_pane::ScriptMsg;
use crate::ui::{ManualMsg, Msg, Probe, cmd};
use xui::widget::DialogAction;

/// Goes to another level with `msg`, discarding unsaved changes if asked to.
pub(super) fn leave(stage: &Stage<'_, Msg>, msg: Msg) {
    stage.emit(msg);
    stage.emit(Msg::Discard(DialogAction::Accept(String::new())));
}

/// Draws the guide, which lays its page out as it paints; whether it shows one.
/// The headless backend paints only for a picture, so one is taken here.
pub(super) fn wait_for_guide(stage: &Stage<'_, Msg>, probe: &Probe) -> bool {
    pump(stage, 1);
    let _ = stage.ui().capture();
    pump(stage, 1);
    probe.guide_ready()
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

/// The guide in the pane's own window: a view of its own, drawn by the
/// window's picture.
pub(super) fn in_window(run: &mut Run, stage: &Stage<'_, Msg>, probe: &Probe) {
    stage.emit(Msg::Menu(cmd::GUIDE));
    pump(stage, 2);
    run.check(probe.guide_ready(), "the guide in the window shows a page");
    let shot = probe
        .script_window()
        .and_then(|w| w.capture().ok())
        .map(|img| img.save_png(run.dir.join("10b-own-window-guide.png")));
    run.check(
        matches!(shot, Some(Ok(()))),
        "snapshot of the guide's window",
    );
}

/// Inserting an example leaves edits unsaved. Going elsewhere then asks, on the
/// default backend too, and whatever was asked for waits for the answer.
pub(super) fn unsaved_edits(run: &mut Run, stage: &Stage<'_, Msg>, probe: &Probe) {
    let dirty = || probe.state().borrow().script.dirty();
    run.check(dirty(), "an inserted example is an unsaved edit");
    let text = probe.code().text();
    stage.emit(Msg::Menu(cmd::NEW));
    pump(stage, 2);
    run.check(probe.code().text() == text, "New waits for the answer");
    run.shot(stage, "13-discard-prompt.png");
    stage.emit(Msg::Discard(DialogAction::Cancel));
    pump(stage, 2);
    run.check(
        probe.code().text() == text && dirty(),
        "keeping the edits keeps them and stays on the level",
    );
    stage.emit(Msg::Menu(cmd::NEW));
    stage.emit(Msg::Discard(DialogAction::Accept(String::new())));
    pump(stage, 2);
    run.check(
        probe.code().text().is_empty() && !dirty(),
        "discarding lets it go ahead",
    );
}

/// The guide on a scaled display, going from page to page as a reader does.
pub(super) fn scaled(run: &mut Run, stage: &Stage<'_, Msg>, probe: &Probe) {
    stage.emit(Msg::Menu(cmd::GUIDE));
    stage.emit(Msg::Script(ScriptMsg::Float(true)));
    stage.emit(Msg::Menu(cmd::GUIDE));
    pump(stage, 2);
    let Some(window) = probe.script_window() else {
        return run.check(false, "the pane has its own window");
    };
    for (topic, name) in [(0, "start"), (1, "names"), (3, "doors"), (7, "reference")] {
        stage.emit(Msg::Script(ScriptMsg::Topic(topic)));
        pump(stage, 2);
        run.check(probe.guide_ready(), &format!("the {name} page is on show"));
        let shot = window
            .capture()
            .map(|img| img.save_png(run.dir.join(format!("{name}.png"))));
        run.check(
            matches!(shot, Ok(Ok(()))),
            &format!("snapshot of the {name} page"),
        );
    }
    stage.emit(Msg::Script(ScriptMsg::Float(false)));
}

/// Draws the manual's page; whether one is on show.
fn wait_for_page(stage: &Stage<'_, Msg>, probe: &Probe) -> bool {
    pump(stage, 1);
    let _ = probe.manual_picture();
    pump(stage, 1);
    probe.manual_ready()
}

/// The window's picture as `name`; whether it was written.
fn snapshot(run: &Run, probe: &Probe, name: &str) -> bool {
    let shot = probe.manual_picture();
    matches!(
        shot.map(|img| img.map(|img| img.save_png(run.dir.join(name)))),
        Some(Ok(Ok(())))
    )
}

pub(super) fn manual(run: &mut Run, stage: &Stage<'_, Msg>, probe: &Probe) {
    probe.state().borrow_mut().set_tool(Tool::Voxel);
    stage.emit(Msg::Menu(cmd::MANUAL));
    pump(stage, 2);
    run.check(
        probe.manual_topic() == Some(manual::topic_for(Tool::Voxel)),
        "F1 opens the manual on the page about the tool in hand",
    );
    let pages = manual::pages();
    for (i, page) in pages.iter().enumerate() {
        stage.emit(Msg::Manual(ManualMsg::Topic(i)));
        let ready = wait_for_page(stage, probe);
        run.check(ready, &format!("the page {:?} is laid out", page.title));
        let name = format!(
            "manual-{i:02}-{}.png",
            page.title.replace([' ', ':', ','], "-")
        );
        run.check(snapshot(run, probe, &name), &format!("snapshot of {name}"));
    }
    probe.state().borrow_mut().set_tool(Tool::Select);
    stage.emit(Msg::Menu(cmd::MANUAL));
    pump(stage, 1);
    run.check(
        probe.manual_topic() == Some(manual::topic_for(Tool::Select)),
        "F1 again turns the open manual to the tool now in hand",
    );
    stage.emit(Msg::Manual(ManualMsg::Close));
    pump(stage, 1);
    run.check(probe.manual_topic().is_none(), "Esc closes the manual");
}
