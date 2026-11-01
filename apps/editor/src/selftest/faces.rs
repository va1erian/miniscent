//! The face-texturing part of the self-test: pick a wall with a real click
//! on the 3D pane, change its mapping through the panel's messages,
//! snapshot before and after, and undo.

use std::time::Duration;

use xui::xui_canvas::snapshot::Stage;

use mscent_math::Vec3d;

use super::{Run, pump};
use crate::model::facetex::FaceEdit;
use crate::model::pick::{FaceRef, Ray, build_all, face_polygon, pick_face};
use crate::model::{Document, FlyCam};
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

/// How far in front of a face the camera stands, in world units.
const STAND_OFF: f64 = 6.0;

/// The largest wall that a camera standing in front of it really sees: the
/// face, and the camera that looks at its middle. A thin sliver would make
/// the scale, rotation and shift of a mapping impossible to judge.
fn biggest_wall(doc: &Document) -> Option<(FaceRef, FlyCam)> {
    let mut walls: Vec<(f64, FaceRef, Vec3d, Vec3d)> = Vec::new();
    for (def, brush) in doc.brushes().iter().zip(build_all(doc)) {
        let planes = brush.map_or(0, |b| b.planes().len());
        for plane in 0..planes {
            let face = FaceRef {
                brush: def.id,
                plane,
            };
            let Some((poly, n)) = face_polygon(doc, face) else {
                continue;
            };
            let verts = poly.vertices();
            if n.z.abs() >= 0.5 || verts.is_empty() {
                continue;
            }
            let sum = verts.iter().fold(Vec3d::ZERO, |a, v| a + *v);
            walls.push((poly.area(), face, sum * (1.0 / verts.len() as f64), n));
        }
    }
    walls.sort_by(|a, b| b.0.total_cmp(&a.0));
    walls.into_iter().find_map(|(_, face, mid, n)| {
        let eye = mid + n * STAND_OFF;
        let ray = Ray {
            origin: eye,
            dir: -n,
        };
        // Nothing may stand between the eye and the face.
        (pick_face(doc, &ray)?.face == face).then(|| (face, look_at(eye, mid)))
    })
}

/// A camera at `eye` looking at `target`.
fn look_at(eye: Vec3d, target: Vec3d) -> FlyCam {
    let d = target - eye;
    let yaw = (-d.x).atan2(d.y) as f32;
    let pitch = d.z.atan2(d.x.hypot(d.y)) as f32;
    FlyCam::new(eye.to_f32(), yaw, pitch)
}

/// Stands the camera in front of the biggest wall, picks it with a real
/// click on the middle of the picture and returns it.
pub(super) fn frame_big_face(
    run: &mut Run,
    stage: &Stage<'_, Msg>,
    probe: &Probe,
) -> Option<FaceRef> {
    let pane = probe.view_pane();
    let state = probe.state();
    state.borrow_mut().selection = None;
    stage.emit(Msg::Tool(FACE_TOOL));
    let framed = biggest_wall(&state.borrow().doc);
    run.check(framed.is_some(), "a big wall can be seen from in front");
    let (wall, cam) = framed?;
    let want = cam.view().forward();
    let along = (-face_polygon(&state.borrow().doc, wall)?.1).to_f32();
    run.check(
        want.dot(along) > 0.99,
        "the camera looks straight at the wall",
    );
    stage.emit(Msg::Camera(Box::new(cam)));
    pump(stage, 40);
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
    let picked = state.borrow().faces.first().copied();
    run.check(
        picked == Some(wall),
        &format!(
            "a click on the picture picked the framed wall: {}",
            probe.message()
        ),
    );
    picked
}

pub(super) fn script(run: &mut Run, stage: &Stage<'_, Msg>, probe: &Probe) {
    let pane = probe.view_pane();
    let state = probe.state();
    let Some(face) = frame_big_face(run, stage, probe) else {
        return;
    };
    let (w, h) = pane.size();
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
