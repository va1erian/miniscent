//! The face-texturing part of the self-test: pick a wall with a real click
//! on the 3D pane, change its mapping through the panel's messages,
//! snapshot before and after, and undo.

use xui::xui_canvas::snapshot::Stage;

use mscent_math::Vec3d;

use super::{Run, pump, wait_for_preview};
use crate::model::facetex::FaceEdit;
use crate::model::pick::{FaceRef, Ray, build_all, face_polygon, pick_face};
use crate::model::{Document, FlyCam};
use crate::studio::Preview;
use crate::ui::facepanel::FaceMsg;
use crate::ui::{Msg, Probe, cmd};

/// The Face tool's index in the palette.
const FACE_TOOL: usize = 7;

fn edit(stage: &Stage<'_, Msg>, e: FaceEdit) {
    stage.emit(Msg::Face(FaceMsg::Edit(e)));
}

/// A change of a parameter must move the face's region by at least this
/// much, in mean absolute difference per byte (0..255).
const MIN_CHANGE: f64 = 6.0;
/// After undo the region may differ by at most this much.
const UNDO_TOLERANCE: f64 = 0.5;

type Pixels = Option<(u32, u32, Vec<u8>)>;

/// Waits for the picture to catch up and lets the new frame show.
fn settle(stage: &Stage<'_, Msg>, probe: &Probe) {
    wait_for_preview(stage, probe);
    pump(stage, 20);
}

/// Mean absolute byte difference over the middle of the picture, where the
/// face is framed and highlighted; `None` if either picture is missing or
/// their sizes differ.
fn centre_diff(a: &Pixels, b: &Pixels) -> Option<f64> {
    let ((w, h, pa), (w2, h2, pb)) = (a.as_ref()?, b.as_ref()?);
    if (w, h) != (w2, h2) || pa.len() != pb.len() {
        return None;
    }
    let (w, h) = (*w as usize, *h as usize);
    let (x0, x1, y0, y1) = (w * 3 / 10, w * 7 / 10, h * 3 / 10, h * 7 / 10);
    let (mut sum, mut n) = (0u64, 0u64);
    for y in y0..y1 {
        for x in x0..x1 {
            let i = (y * w + x) * 4;
            for c in 0..3 {
                sum += u64::from(pa[i + c].abs_diff(pb[i + c]));
                n += 1;
            }
        }
    }
    (n > 0).then(|| sum as f64 / n as f64)
}

/// Checks that the picture now differs visibly from `before` in the face's
/// region (unless the adaptive size changed, which cannot be compared).
fn parameter_changed(run: &mut Run, before: &Pixels, now: &Pixels, what: &str) {
    match centre_diff(before, now) {
        Some(d) => run.check(
            d >= MIN_CHANGE,
            &format!("{what} visibly changes the face (mean diff {d:.2})"),
        ),
        None => println!("skip {what}: picture size changed"),
    }
}

/// Half the vertical field of view's tangent (the renderer's 1.3 rad).
const HALF_TAN: f64 = 0.7602;
/// The narrowest picture the face must fit into, width over height.
const MIN_ASPECT: f64 = 2.5;
/// Farthest the camera stands from a wall, in world units.
const MAX_STAND_OFF: f64 = 22.0;

/// The largest wall that a camera standing back far enough to see all of it,
/// with room around it for its highlight, really sees: the face, and the
/// camera that looks at its middle. A thin sliver would make the scale,
/// rotation and shift of a mapping impossible to judge.
fn biggest_wall(doc: &Document) -> Option<(FaceRef, FlyCam)> {
    let mut walls: Vec<(f64, FaceRef, Vec3d, Vec3d, f64)> = Vec::new();
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
            let mid = verts.iter().fold(Vec3d::ZERO, |a, v| a + *v) * (1.0 / verts.len() as f64);
            let (mut half_h, mut half_w) = (0.0f64, 0.0f64);
            for v in verts {
                let d = *v - mid;
                half_h = half_h.max(d.z.abs());
                half_w = half_w.max((d.x * n.y - d.y * n.x).abs());
            }
            let stand = (half_h / HALF_TAN).max(half_w / (HALF_TAN * MIN_ASPECT)) * 1.35;
            walls.push((poly.area(), face, mid, n, stand));
        }
    }
    walls.sort_by(|a, b| b.0.total_cmp(&a.0));
    walls.into_iter().find_map(|(_, face, mid, n, stand)| {
        let eye = mid + n * stand.clamp(3.0, MAX_STAND_OFF);
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

/// Scale 2x (0.125 units per texel), rotate 45 degrees, shift.
pub(super) fn apply_mapping(stage: &Stage<'_, Msg>) {
    edit(stage, FaceEdit::ScaleU(0.125));
    edit(stage, FaceEdit::ScaleV(0.125));
    edit(stage, FaceEdit::Rotation(45.0));
    edit(stage, FaceEdit::ShiftU(24.0));
    edit(stage, FaceEdit::ShiftV(8.0));
}

/// The wide window's before/after pair of the picked face.
pub(super) fn wide_edit(run: &mut Run, stage: &Stage<'_, Msg>, probe: &Probe) {
    apply_mapping(stage);
    let p = wait_for_preview(stage, probe);
    run.check(
        p == Preview::Full,
        &format!("wide: new mapping shown: {p:?}"),
    );
    pump(stage, 20);
    run.shot(stage, "wide-03-face-edited.png");
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
    // Brick has clear horizontal courses: the scale, rotation and shift of
    // the edit are unmistakable on it, unlike on a noisy rock texture.
    edit(stage, FaceEdit::Material(mscent_world::materials::BRICK));
    settle(stage, probe);
    run.shot(stage, "08-face-picked.png");
    let picture_before = pane.frame_checksum();
    let pixels_before = pane.frame_pixels();
    let original = state.borrow().face_texinfo(face);
    let depth = state.borrow().history.depth();
    let Some(orig) = original else {
        run.check(false, "the picked face has a mapping");
        return;
    };

    // Each parameter alone, so its own effect is visible in a picture.
    edit(stage, FaceEdit::Rotation(45.0));
    settle(stage, probe);
    run.shot(stage, "09a-face-rotated-only.png");
    parameter_changed(run, &pixels_before, &pane.frame_pixels(), "rotation");
    edit(stage, FaceEdit::Rotation(orig.rotation_deg));
    edit(stage, FaceEdit::ShiftU(24.0));
    edit(stage, FaceEdit::ShiftV(8.0));
    settle(stage, probe);
    run.shot(stage, "09b-face-shifted-only.png");
    parameter_changed(run, &pixels_before, &pane.frame_pixels(), "shift");
    edit(stage, FaceEdit::ShiftU(orig.u_shift));
    edit(stage, FaceEdit::ShiftV(orig.v_shift));
    edit(stage, FaceEdit::ScaleU(0.125));
    edit(stage, FaceEdit::ScaleV(0.125));
    settle(stage, probe);
    run.shot(stage, "09c-face-scaled-only.png");
    parameter_changed(run, &pixels_before, &pane.frame_pixels(), "scale");
    edit(stage, FaceEdit::ScaleU(orig.u_scale));
    edit(stage, FaceEdit::ScaleV(orig.v_scale));

    apply_mapping(stage);
    let p = wait_for_preview(stage, probe);
    run.check(
        p == Preview::Full,
        &format!("the new mapping is shown: {p:?}"),
    );
    pump(stage, 20);
    let picture_after = pane.frame_checksum();
    run.check(
        picture_before.is_some() && picture_before != picture_after,
        "the 3D picture changed with the mapping",
    );
    run.shot(stage, "09-face-edited.png");
    parameter_changed(
        run,
        &pixels_before,
        &pane.frame_pixels(),
        "combined mapping",
    );
    let edited = state.borrow().face_texinfo(face);
    run.check(
        edited.is_some_and(|t| t.rotation_deg == 45.0 && t.u_scale == 0.125 && t.v_shift == 8.0),
        "the document holds the new mapping",
    );

    // Undo every step of the edit: the mapping and the picture return.
    while state.borrow().history.depth() > depth {
        stage.emit(Msg::Menu(cmd::UNDO));
    }
    settle(stage, probe);
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
    if let (Some(d), true) = (
        centre_diff(&pixels_before, &pane.frame_pixels()),
        comparable,
    ) {
        run.check(
            d <= UNDO_TOLERANCE,
            &format!("after undo the face region equals the before (mean diff {d:.3})"),
        );
    }
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
