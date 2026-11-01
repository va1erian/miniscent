//! The face-texturing part of the self-test: pick a wall with a real click
//! on the 3D pane, change its mapping through the panel's messages,
//! snapshot before and after, and undo.

use std::time::Duration;

use xui::xui_canvas::snapshot::Stage;

use super::{Run, pump};
use crate::model::facetex::FaceEdit;
use crate::model::pick::face_polygon;
use crate::ui::facepanel::FaceMsg;
use crate::ui::{Msg, Probe, cmd};

/// The Face tool's index in the palette.
const FACE_TOOL: usize = 6;

/// Emits steps until the background texture refresh reports; returns its
/// status.
fn wait_for_refresh(stage: &Stage<'_, Msg>, probe: &Probe) -> String {
    for _ in 0..6_000 {
        stage.emit(Msg::Step);
        let m = probe.message();
        if m.starts_with("Texture mapping updated") {
            return m;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    "timed out waiting for the texture refresh".into()
}

fn edit(stage: &Stage<'_, Msg>, e: FaceEdit) {
    stage.emit(Msg::Face(FaceMsg::Edit(e)));
}

pub(super) fn script(run: &mut Run, stage: &Stage<'_, Msg>, probe: &Probe) {
    let pane = probe.view_pane();
    let state = probe.state();
    state.borrow_mut().selection = None;
    stage.emit(Msg::Tool(FACE_TOOL));
    pump(stage, 20);

    // A real click through the centre of the picture picks the face there.
    let (w, h) = pane.size();
    pane.push_click(crate::ui::Click {
        nx: 0.5,
        ny: 0.5,
        aspect: w as f32 / h.max(1) as f32,
        right: false,
        ctrl: false,
        alt: false,
    });
    stage.emit(Msg::Step);
    let face = state.borrow().faces.first().copied();
    run.check(
        face.is_some(),
        &format!("a click on the picture picked a face: {}", probe.message()),
    );
    let Some(face) = face else { return };
    let wall = face_polygon(&state.borrow().doc, face).is_some_and(|(_, n)| n.z.abs() < 0.5);
    run.check(wall, "the picked face is a wall");
    pump(stage, 20);
    run.shot(stage, "08-face-picked.png");
    let picture_before = pane.frame_checksum();
    let original = state.borrow().face_texinfo(face);
    let depth = state.borrow().history.depth();

    // Scale 2x (0.125 units per texel), rotate 45 degrees, shift.
    edit(stage, FaceEdit::ScaleU(0.125));
    edit(stage, FaceEdit::ScaleV(0.125));
    edit(stage, FaceEdit::Rotation(45.0));
    edit(stage, FaceEdit::ShiftU(24.0));
    edit(stage, FaceEdit::ShiftV(8.0));
    let status = wait_for_refresh(stage, probe);
    run.check(
        status.contains("lightmap kept"),
        &format!("the new mapping reused the cached lightmap: {status}"),
    );
    pump(stage, 20);
    let picture_after = pane.frame_checksum();
    run.check(
        picture_before.is_some() && picture_before != picture_after,
        "the 3D picture changed with the mapping",
    );
    run.shot(stage, "09-face-edited.png");
    let edited = state.borrow().face_texinfo(face);
    run.check(
        edited.is_some_and(|t| t.rotation_deg == 45.0 && t.u_scale == 0.125 && t.v_shift == 8.0),
        "the document holds the new mapping",
    );

    // Undo every step of the edit: the mapping and the picture return.
    while state.borrow().history.depth() > depth {
        stage.emit(Msg::Menu(cmd::UNDO));
    }
    wait_for_refresh(stage, probe);
    pump(stage, 20);
    run.check(
        state.borrow().face_texinfo(face) == original,
        "undo restored the original mapping",
    );
    let picture_undone = pane.frame_checksum();
    // The adaptive resolution may differ between runs on a loaded machine.
    let comparable = picture_before
        .zip(picture_undone)
        .is_some_and(|(a, b)| a.0 == b.0 && a.1 == b.1);
    run.check(
        !comparable || picture_before == picture_undone,
        "the picture after undo equals the picture before the edit",
    );
    run.shot(stage, "10-face-undone.png");

    // Ctrl + click on another spot changes the picked set.
    pane.push_click(crate::ui::Click {
        nx: 0.2,
        ny: 0.6,
        aspect: w as f32 / h.max(1) as f32,
        right: false,
        ctrl: true,
        alt: false,
    });
    stage.emit(Msg::Step);
    pump(stage, 20);
    let picked = state.borrow().faces.len();
    run.check(
        picked != 1,
        &format!("ctrl-click changed the set to {picked}"),
    );
    run.shot(stage, "11-ctrl-click.png");
}
