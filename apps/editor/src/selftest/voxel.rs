//! Selftest of the Voxel tool: a stroke on a wall changes the picture with no
//! preview job, undo gives back the same picture, and the sculpted level plays.

use mscent_world::level_text;
use xui::xui_canvas::snapshot::Stage;

use super::{Run, pump, wait_for_preview};
use crate::model::Tool;
use crate::studio::Preview;
use crate::ui::{Click, Msg, PaneDrag, Probe, cmd};

const LEVEL: &str = "miniscent-level 1
box subtract 0 4  -10 -10 0  10 10 6
player 0 -8 1 0
goal 0 8 1 1
light 0 0 4  1 0.95 0.8  30 3
";

/// Ticks until the picture is current and fully lit, and the frame is shown.
fn settled_picture(stage: &Stage<'_, Msg>, probe: &Probe) -> Option<(u32, u32, u64)> {
    for _ in 0..1500 {
        pump(stage, 1);
        if probe.preview() == Preview::Full {
            break;
        }
    }
    pump(stage, 12);
    probe.view_pane().frame_checksum()
}

fn click(nx: f32, ny: f32) -> Click {
    Click {
        nx,
        ny,
        aspect: 1.6,
        right: false,
        ctrl: false,
        alt: false,
    }
}

fn drag(nx: f32, ny: f32, released: bool) -> PaneDrag {
    PaneDrag {
        nx,
        ny,
        aspect: 1.6,
        released,
    }
}

pub(super) fn script(run: &mut Run, stage: &Stage<'_, Msg>, probe: &Probe) {
    let file = run.dir.join("sculpted.msl");
    let level = level_text::from_text(LEVEL).expect("the level text");
    std::fs::write(&file, mscent_world::format::write(&level)).expect("writes");
    stage.emit(Msg::OpenPath(file.clone()));
    wait_for_preview(stage, probe);
    let original = settled_picture(stage, probe);
    run.check(original.is_some(), "the room is on screen");
    run.shot(stage, "voxel-01-room.png");

    probe.state().borrow_mut().tool = Tool::Voxel;
    pump(stage, 2);
    run.shot(stage, "voxel-02-tool.png");
    let pane = probe.view_pane();
    pane.push_click(click(0.5, 0.5));
    pump(stage, 2);
    run.check(
        probe.state().borrow().stroking(),
        "the press starts a stroke",
    );
    for i in 1..12 {
        pane.push_drag(drag(0.5 + i as f32 * 0.015, 0.5, false));
        pump(stage, 2);
    }
    pane.push_drag(drag(0.68, 0.5, true));
    pump(stage, 8);
    run.check(
        !probe.state().borrow().stroking(),
        &format!("the stroke ended: {}", probe.message()),
    );
    let volumes = probe.state().borrow().doc.volumes().count();
    run.check(volumes == 1, &format!("{volumes} volumes after the stroke"));
    let sculpted = settled_picture(stage, probe);
    run.check(
        sculpted.is_some() && sculpted != original,
        "the stroke changed the picture",
    );
    run.shot(stage, "voxel-03-sculpted.png");

    stage.emit(Msg::Menu(cmd::UNDO));
    let undone = settled_picture(stage, probe);
    run.check(undone == original, "undo gives back the same picture");
    stage.emit(Msg::Menu(cmd::REDO));
    let redone = settled_picture(stage, probe);
    run.check(redone == sculpted, "redo gives back the sculpted picture");

    stage.emit(Msg::SavePath(file.clone()));
    stage.emit(Msg::OpenPath(file.clone()));
    let reopened = probe.state().borrow().doc.volumes().count();
    run.check(reopened == 1, "the volume survives a save and reopen");
    wait_for_preview(stage, probe);
    stage.emit(Msg::Menu(cmd::PLAY));
    run.check(
        probe.message().starts_with("Playing"),
        &format!("the sculpted level plays: {}", probe.message()),
    );
    pump(stage, 20);
    stage.emit(Msg::Escape);
    pump(stage, 2);
}
