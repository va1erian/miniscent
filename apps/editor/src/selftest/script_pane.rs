//! Selftest of the script workflow, through real key events and the editor's own
//! messages: open a sample, type with completion, see a mistake in the problem
//! list, play what was typed, add a basic entity, read the guide.

use std::path::Path;
use std::time::{Duration, Instant};

use xui::backend::Event;
use xui::xui_canvas::snapshot::Stage;
use xui::{Key, Modifiers};

use super::{Run, guide, pump, wait_for_preview};
use crate::model::Selection;
use crate::script::kit::{KIT, KitKind};
use crate::ui::script_pane::ScriptMsg;
use crate::ui::{Msg, Probe, cmd};

/// A key as the window system delivers it.
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

/// Types as a keyboard does: Return is a key, and the rest are characters.
fn type_text(stage: &Stage<'_, Msg>, text: &str) {
    for c in text.chars() {
        if c == '\n' {
            press(stage, Key::RETURN);
        } else {
            stage.inject(Event::Char(c));
        }
    }
}

fn caret_to_end(probe: &Probe) {
    let code = probe.code();
    code.set_caret(code.text().chars().count());
    code.focus();
}

/// Ticks until the problem list's first row says `wanted`.
fn wait_for_problem(stage: &Stage<'_, Msg>, probe: &Probe, wanted: &str) -> String {
    let end = Instant::now() + Duration::from_secs(30);
    loop {
        stage.emit(Msg::Step);
        let row = probe.problem_row();
        if row.contains(wanted) || Instant::now() > end {
            return row;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn wait_for_prefix(stage: &Stage<'_, Msg>, probe: &Probe, prefix: &str) -> String {
    let end = Instant::now() + Duration::from_secs(30);
    loop {
        stage.emit(Msg::Step);
        let m = probe.message();
        if m.starts_with(prefix) || Instant::now() > end {
            return m;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn sample_script(probe: &Probe) -> std::path::PathBuf {
    let state = probe.state();
    let st = state.borrow();
    let level = st.doc.level_path().unwrap_or(Path::new(""));
    level.with_extension("st")
}

/// Opening a sample shows its level and script, and it plays at once.
fn sample(run: &mut Run, stage: &Stage<'_, Msg>, probe: &Probe) {
    stage.emit(Msg::Script(ScriptMsg::Sample(0)));
    pump(stage, 3);
    let text = probe.code().text();
    run.check(
        text.contains("autoDoor: 'gate'"),
        "opening a sample puts its script in the code editor",
    );
    run.check(probe.script_open(), "a level with a script opens the pane");
    let row = wait_for_problem(stage, probe, "No problems");
    run.check(row == "No problems", &format!("the sample is clean: {row}"));
    wait_for_preview(stage, probe);
    run.shot(stage, "01-sample.png");
    stage.emit(Msg::Menu(cmd::PLAY));
    pump(stage, 5);
    run.check(
        probe.message().starts_with("Playing"),
        &format!("a sample plays at once: {}", probe.message()),
    );
    stage.emit(Msg::Escape);
    pump(stage, 2);
}

/// Typing in the pane: letters are text and not tool shortcuts, the popup offers
/// what fits, and accepting it inserts a whole message.
fn typing(run: &mut Run, stage: &Stage<'_, Msg>, probe: &Probe) {
    let tool = probe.state().borrow().tool;
    let depth = probe.state().borrow().history.depth();
    let brushes = probe.state().borrow().doc.brushes().len();
    let first = probe.state().borrow().doc.brushes()[0].id;
    probe.state().borrow_mut().selection = Some(Selection::Brush(first));
    caret_to_end(probe);
    // A letter arrives as a key and then a character; the key is a shortcut
    // anywhere else, and Delete would delete the selected brush.
    press(stage, Key::B);
    press(stage, Key::DELETE);
    run.check(
        probe.state().borrow().doc.brushes().len() == brushes,
        "Delete in the pane leaves the selected brush alone",
    );
    probe.state().borrow_mut().selection = None;
    type_text(stage, "\ngate := mover: '");
    pump(stage, 1);
    let code = probe.code();
    run.check(code.is_completing(), "a quote after mover: opens the popup");
    run.shot(stage, "02-names.png");
    press(stage, Key::RETURN);
    let text = code.text();
    run.check(
        text.contains("gate := mover: 'gate"),
        &format!("accepting a name inserts it: {}", tail(&text)),
    );
    run.check(!code.is_completing(), "accepting closes the popup");
    type_text(stage, "'.");
    run.check(
        !code.is_completing(),
        "the period ending a statement opens nothing",
    );
    type_text(stage, "\ngate mo");
    pump(stage, 1);
    run.check(code.is_completing(), "two letters of a word open the popup");
    run.shot(stage, "03-messages.png");
    code.close_completion();
    press(stage, Key::BACK);
    press(stage, Key::BACK);
    type_text(stage, "poseNow.\n");
    pump(stage, 2);
    run.check(
        code.text().ends_with("gate poseNow.\n"),
        &format!(
            "Return after a statement is a new line: {}",
            tail(&code.text())
        ),
    );
    let state = probe.state();
    let st = state.borrow();
    run.check(st.tool == tool, "letters typed in the pane pick no tool");
    run.check(
        st.history.depth() == depth,
        "typing leaves the level's undo alone",
    );
    run.check(
        st.script.text() == code.text() && st.script.dirty(),
        "the model follows the widget, and knows it is not saved",
    );
}

fn tail(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    chars[chars.len().saturating_sub(50)..].iter().collect()
}

/// A mistake shows on its line with the level as it is, Play refuses it, and the
/// list takes you to it. F5 reads the newest text, not the last saved.
fn mistakes(run: &mut Run, stage: &Stage<'_, Msg>, probe: &Probe) {
    caret_to_end(probe);
    type_text(stage, "(mover: 'gaet') moving.\n");
    let row = wait_for_problem(stage, probe, "Line");
    run.check(
        row.contains("Line") && row.contains("gaet"),
        &format!("a misspelt name is reported as it is typed: {row}"),
    );
    run.shot(stage, "04-problem.png");
    let code = probe.code();
    code.goto(0, 0);
    stage.emit(Msg::Script(ScriptMsg::Problem(0)));
    let (line, _) = code.caret_line_col();
    run.check(
        line > 3,
        &format!("a problem's row goes to its line: {line}"),
    );

    stage.emit(Msg::Menu(cmd::PLAY));
    let message = wait_for_prefix(stage, probe, "Cannot play");
    run.check(
        message.contains("first_door.st:"),
        &format!("F5 tries the text being edited: {message}"),
    );
    let on_disk = std::fs::read_to_string(sample_script(probe)).unwrap_or_default();
    // A sample checked out on Windows is CRLF, and the file keeps what it had.
    run.check(
        on_disk.replace("\r\n", "\n") == code.text(),
        "F5 saved the script first",
    );

    // The same undo Ctrl+Z gets: the code editor's, not the level's.
    let depth = probe.state().borrow().history.depth();
    code.focus();
    stage.emit(Msg::Guarded(Box::new(Msg::Menu(cmd::UNDO))));
    run.check(
        probe.state().borrow().history.depth() == depth,
        "Ctrl+Z in the pane leaves the level alone",
    );

    let fixed = code.text().replace("gaet", "gate");
    code.set_text(&fixed);
    stage.emit(Msg::Script(ScriptMsg::Edited));
    let row = wait_for_problem(stage, probe, "No problems");
    run.check(row == "No problems", &format!("fixing it clears it: {row}"));
    stage.emit(Msg::Menu(cmd::PLAY));
    let message = wait_for_prefix(stage, probe, "Playing");
    run.check(
        message.starts_with("Playing"),
        &format!("then it plays: {message}"),
    );
    stage.emit(Msg::Escape);
    pump(stage, 2);
}

/// A change in another program: taken in when clean, a choice when not.
fn outside(run: &mut Run, stage: &Stage<'_, Msg>, probe: &Probe) {
    let file = sample_script(probe);
    let code = probe.code();
    let ours = code.text();
    std::fs::write(&file, "say: 'from outside'.\n").expect("writes");
    let end = Instant::now() + Duration::from_secs(5);
    while code.text() == ours && Instant::now() < end {
        stage.emit(Msg::Step);
        std::thread::sleep(Duration::from_millis(20));
    }
    run.check(
        code.text().contains("from outside"),
        "a clean buffer follows the file",
    );
    caret_to_end(probe);
    type_text(stage, "win.\n");
    std::fs::write(&file, "say: 'again'.\n").expect("writes");
    let state = probe.state();
    let conflicted = || state.borrow().script.conflict();
    let end = Instant::now() + Duration::from_secs(5);
    while !conflicted() && Instant::now() < end {
        stage.emit(Msg::Step);
        std::thread::sleep(Duration::from_millis(20));
    }
    run.check(
        conflicted(),
        "edits are not overwritten by a change outside",
    );
    stage.emit(Msg::Menu(cmd::PLAY));
    run.check(
        probe.message().contains("changed on disk"),
        &format!("F5 does not overwrite it either: {}", probe.message()),
    );
    let kept = std::fs::read_to_string(&file).unwrap_or_default();
    run.check(kept == "say: 'again'.\n", "and the file is as it was");
    run.check(code.text().contains("win."), "the edits are still there");
    run.shot(stage, "05-conflict.png");
    stage.emit(Msg::Script(ScriptMsg::Reload));
    pump(stage, 1);
    run.check(code.text() == "say: 'again'.\n", "Reload takes the file");
    run.check(!conflicted(), "and that settles it");
}

/// Add entity: pick, click in a view, get a named door and its script.
fn kit(run: &mut Run, stage: &Stage<'_, Msg>, probe: &Probe) {
    let door = KIT
        .iter()
        .position(|e| e.kind == KitKind::Door)
        .unwrap_or(0);
    stage.emit(Msg::Script(ScriptMsg::Kit(door)));
    press(stage, Key::ESCAPE);
    run.check(
        probe.state().borrow().tool == crate::model::Tool::Select,
        "Esc changes your mind",
    );
    // The real button and the real top view, as a mouse reaches them.
    let (button, view) = ((1205, 527), (220, 132));
    stage.click(button.0, button.1);
    run.check(
        probe.message().contains("Click"),
        &format!("picking one says what to do: {}", probe.message()),
    );
    stage.click(view.0, view.1);
    pump(stage, 2);
    let text = probe.code().text();
    run.check(
        text.contains("autoDoor: 'door_1'"),
        &format!("its lines go in the script: {}", tail(&text)),
    );
    let state = probe.state();
    let selected = matches!(state.borrow().selection, Some(Selection::Brush(_)));
    run.check(selected, "the door is selected, to move and resize");
    let groups = state.borrow().doc.groups().to_vec();
    run.check(groups.iter().any(|g| g.name == "door_1"), "it is a mover");
    let row = wait_for_problem(stage, probe, "No problems");
    run.check(row == "No problems", &format!("it checks clean: {row}"));
    run.shot(stage, "06-kit.png");
    stage.emit(Msg::Menu(cmd::SAVE));
    run.check(
        !state.borrow().script.dirty(),
        "File > Save writes the script with the level",
    );
}

/// A level with no file has nowhere to keep a script, so F5 asks for one.
fn unsaved(run: &mut Run, stage: &Stage<'_, Msg>, probe: &Probe) {
    stage.emit(Msg::Menu(cmd::SAVE));
    stage.emit(Msg::Menu(cmd::NEW));
    pump(stage, 1);
    run.check(
        probe.code().text().is_empty(),
        "a new level has an empty script",
    );
    caret_to_end(probe);
    type_text(stage, "win.");
    stage.emit(Msg::Menu(cmd::PLAY));
    run.check(
        probe.message().starts_with("Save the level first"),
        &format!("F5 asks for a file: {}", probe.message()),
    );
}

/// The pane in a window of its own: same text, same model, and back again.
fn own_window(run: &mut Run, stage: &Stage<'_, Msg>, probe: &Probe) {
    let text = probe.code().text();
    stage.emit(Msg::Script(ScriptMsg::Float(true)));
    pump(stage, 2);
    let Some(window) = probe.script_window() else {
        run.check(false, "the pane opens in a window of its own");
        return;
    };
    run.check(probe.code().text() == text, "the window has the script");
    let shot = window
        .capture()
        .map(|img| img.save_png(run.dir.join("10-own-window.png")));
    run.check(
        matches!(shot, Ok(Ok(()))),
        &format!("snapshot of the window {:?}", shot.is_ok()),
    );
    run.shot(stage, "11-views-without-pane.png");

    // The model follows the window's widget. The headless backend has no wake, so
    // a second window's own queue does not run here, and the message comes from
    // the main window: the forward from one to the other needs a real window.
    let code = probe.code();
    code.set_caret(code.text().chars().count());
    code.insert_text("say: 'typed in the window'.\n");
    stage.emit(Msg::Script(ScriptMsg::Edited));
    run.check(
        probe
            .state()
            .borrow()
            .script
            .text()
            .contains("in the window"),
        "what is typed in the window reaches the model",
    );

    // A basic entity picked while it is out goes in the window's text.
    let door = KIT
        .iter()
        .position(|e| e.kind == KitKind::Trigger)
        .unwrap_or(0);
    stage.emit(Msg::Script(ScriptMsg::Kit(door)));
    stage.click(220, 132);
    pump(stage, 2);
    run.check(
        probe.code().text().contains("onceEnter"),
        "an entity placed goes in the window's text",
    );

    // Hidden and shown again, with an entity placed while it is hidden: the set
    // of widgets not in use must not put old text back in the model.
    let before = probe.code().text();
    stage.emit(Msg::Menu(cmd::SCRIPT_PANE));
    pump(stage, 2);
    run.check(probe.script_window().is_none(), "F6 hides the window");
    probe
        .state()
        .borrow_mut()
        .place_kit(KitKind::Exit, [0.0, 0.0, 0.0]);
    pump(stage, 2);
    let after = probe.code().text();
    run.check(
        probe.script_window().is_some(),
        "placing something shows its script again",
    );
    let (at, _) = probe.code().caret_line_col();
    run.check(
        after.lines().nth(at).is_some_and(|l| l.contains("Exit")),
        &format!("with the caret on it, in the window: line {at}"),
    );
    run.check(
        after.starts_with(&before) && after.matches("exitZone").count() >= 1,
        &format!("nothing is lost or doubled: {}", tail(&after)),
    );
    run.check(
        probe.state().borrow().script.text() == after,
        "the model and the window agree",
    );

    // Play takes the keyboard, so the window makes way and comes back.
    stage.emit(Msg::Script(ScriptMsg::Save));
    wait_for_preview(stage, probe);
    stage.emit(Msg::Menu(cmd::PLAY));
    let message = wait_for_prefix(stage, probe, "Playing");
    run.check(message.starts_with("Playing"), &format!("plays: {message}"));
    run.check(
        probe.script_window().is_none(),
        "the window makes way for Play",
    );
    stage.emit(Msg::Escape);
    pump(stage, 2);
    run.check(probe.script_window().is_some(), "and comes back after");
    run.check(
        probe.code().text().contains("in the window"),
        "with the text it had",
    );
    guide::in_window(run, stage, probe);

    let text = probe.code().text();
    let caret = probe.code().caret_line_col();
    stage.emit(Msg::Script(ScriptMsg::Float(false)));
    pump(stage, 2);
    run.check(
        probe.code().caret_line_col() == caret,
        "docking keeps the caret",
    );
    run.check(probe.script_window().is_none(), "docking closes the window");
    run.check(probe.script_open(), "and the pane is on show");
    run.check(probe.code().text() == text, "with the same text");
    run.shot(stage, "12-docked-again.png");
}

pub(super) fn script(run: &mut Run, stage: &Stage<'_, Msg>, probe: &Probe) {
    super::level_settings::script(run, stage, probe);
    sample(run, stage, probe);
    typing(run, stage, probe);
    mistakes(run, stage, probe);
    outside(run, stage, probe);
    kit(run, stage, probe);
    own_window(run, stage, probe);
    guide::guide(run, stage, probe);
    guide::unsaved_edits(run, stage, probe);
    // Last: its Save dialog stays up, as nobody is there to answer it.
    unsaved(run, stage, probe);
}
