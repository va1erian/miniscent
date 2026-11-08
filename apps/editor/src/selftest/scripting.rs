//! Selftest of level scripting in the editor: name things, make a mover, save,
//! play with a script beside the level, and be told when the script is broken.

use std::time::{Duration, Instant};

use mscent_world::level_text;
use xui::xui_canvas::snapshot::Stage;

use super::{Run, pump, wait_for_preview};
use crate::model::{EntityKind, Selection};
use crate::ui::{Msg, Probe, Prop, cmd};

const LEVEL: &str = "miniscent-level 1
box subtract 0 1  -10 -10 -5  10 20 5
box add 1 2  -3 8 -5  3 9 5
player 0 0 0 0
light 0 3 4  1 0.95 0.8  30 3
";

const SCRIPT: &str = "
door := mover: 'door'.
door pose: 'open' move: (x: 0 y: 0 z: 9).
after: 2 do: [ door moveTo: 'open' in: 0.2. say: 'opening' ].
spawn: 'terminal' at: (marker: 'spot') tag: 't'.
";

/// A script with no door opening, for reloading while playing.
const STILL: &str = "(mover: 'door') pose: 'open' move: (x: 0 y: 0 z: 9).
";

/// Whether the door, a glowing panel, is in view in the Play pane. The picture's
/// size follows the frame rate, so this samples by proportion and by hue.
fn door_ahead(probe: &Probe) -> bool {
    let Some((w, h, px)) = probe.play_pane().frame_pixels() else {
        return false;
    };
    let (w, h) = (w as usize, h as usize);
    let at = (h * 27 / 100 * w + w * 2 / 5) * 4;
    // The panel is warm, and the wall behind it blue-grey.
    px.get(at..at + 3)
        .is_some_and(|c| c[0] > c[2].saturating_add(8))
}

/// Lets the game run for `seconds`.
fn run_for(stage: &Stage<'_, Msg>, seconds: f32) {
    let end = Instant::now() + Duration::from_secs_f32(seconds);
    while Instant::now() < end {
        stage.emit(Msg::Step);
        std::thread::sleep(Duration::from_millis(8));
    }
    pump(stage, 2);
}

fn name_selection(stage: &Stage<'_, Msg>, name: &str) {
    stage.emit(Msg::Prop(Prop::NameText(name.to_string())));
    stage.emit(Msg::Prop(Prop::ApplyNames));
}

pub(super) fn script(run: &mut Run, stage: &Stage<'_, Msg>, probe: &Probe) {
    let file = run.dir.join("scripted.msl");
    let script_file = file.with_extension("st");
    let level = level_text::from_text(LEVEL).expect("the level text");
    std::fs::write(&file, mscent_world::format::write(&level)).expect("writes");
    let _ = std::fs::remove_file(&script_file);
    super::guide::leave(stage, Msg::OpenPath(file.clone()));
    pump(stage, 3);

    // Make the second brush a mover, and name it, through the panel's messages.
    let door = probe.state().borrow().doc.brushes()[1].id;
    probe.state().borrow_mut().selection = Some(Selection::Brush(door));
    stage.emit(Msg::Menu(cmd::MAKE_MOVER));
    run.check(
        probe.message().contains("mover_1"),
        &format!("make mover: {}", probe.message()),
    );
    pump(stage, 2);
    name_selection(stage, "door");
    let groups = probe
        .state()
        .borrow()
        .doc
        .to_level()
        .map(|l| l.mover_groups);
    run.check(
        groups.is_ok_and(|g| g.len() == 1 && g[0].name == "door"),
        "the brush is the mover 'door'",
    );

    // A marker, placed as the tool's click places it, and named.
    let spot = probe
        .state()
        .borrow_mut()
        .place_entity(EntityKind::Marker, [0.0, 4.0, 0.0]);
    probe.state().borrow_mut().selection = Some(Selection::Entity(spot));
    pump(stage, 2);
    name_selection(stage, "spot");
    name_selection(stage, "Not Valid");
    run.check(
        probe.message().contains("lower-case"),
        &format!("a bad name is refused: {}", probe.message()),
    );
    run.shot(stage, "01-mover-named.png");

    // Saving keeps it all.
    stage.emit(Msg::SavePath(file.clone()));
    super::guide::leave(stage, Msg::OpenPath(file.clone()));
    let reopened = probe.state().borrow().doc.to_level();
    run.check(
        reopened.is_ok_and(|l| l.mover_groups.len() == 1 && l.labels.len() == 1),
        "names and movers survive a save and reopen",
    );

    // A broken script stops Play, and says where.
    std::fs::write(&script_file, "(mover: 'dor') moveTo: 'x' in: 1.").expect("writes");
    super::guide::leave(stage, Msg::OpenPath(file.clone()));
    wait_for_preview(stage, probe);
    stage.emit(Msg::Menu(cmd::PLAY));
    run.check(
        probe.message().contains("scripted.st:1:"),
        &format!("a broken script is reported: {}", probe.message()),
    );

    // Fixing it, with no help from the editor, lets it play: the door moves.
    std::fs::write(&script_file, SCRIPT).expect("writes");
    run_for(stage, 1.5);
    wait_for_preview(stage, probe);
    stage.emit(Msg::Menu(cmd::PLAY));
    run.check(
        probe.message().starts_with("Playing"),
        &format!("plays with a script: {}", probe.message()),
    );
    run_for(stage, 0.5);
    run.check(door_ahead(probe), "the door starts shut");
    run.shot(stage, "02-door-shut.png");
    run_for(stage, 2.5);
    run.check(!door_ahead(probe), "the script opened the door");
    run.shot(stage, "03-door-open.png");

    // Saving another script while playing starts over on it: the door is shut.
    std::fs::write(&script_file, STILL).expect("writes");
    run_for(stage, 1.0);
    run.check(door_ahead(probe), "a script saved during Play is picked up");
    stage.emit(Msg::Escape);
    let _ = std::fs::remove_file(&script_file);
}
