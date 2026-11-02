//! Selftest of vistas: New Vista, a play-space wall painted with a View
//! material through the face panel's messages, the sky read from the 3D picture
//! and from Play, a slot with no layer showing the gradient, and undo giving the
//! starting picture back.

use mscent_math::Vec3d;
use mscent_render::BuildConfig;
use mscent_world::materials::view_material;
use mscent_world::{Entity, level_text};
use xui::xui_canvas::snapshot::Stage;

use super::faces::{FACE_TOOL, frame_wall};
use super::{Run, pump, wait_for_message, wait_for_preview};
use crate::model::facetex::FaceEdit;
use crate::model::pick::FaceRef;
use crate::model::{EditorState, Selection};
use crate::studio::Preview;
use crate::ui::facepanel::FaceMsg;
use crate::ui::{Click, Msg, Probe, Prop, cmd};

/// A room 24 by 24 by 10. The player stands in it facing -x (a yaw of a quarter
/// turn), where the wall is the one the test paints.
const LEVEL: &str = "miniscent-level 1
box subtract 0 0  -12 -12 0  12 12 10
player 4 0 2 1.5707964
goal 8 8 1 1
light 0 0 8  1 0.95 0.8  40 3
";

/// Pixels read: width, height and RGBA bytes.
type Pixels = (u32, u32, Vec<u8>);

/// A change in the wall's region of at least this much (mean absolute byte
/// difference) is a different wall.
const MIN_CHANGE: f64 = 12.0;
/// After undo the region may differ by at most this much.
const UNDO_TOLERANCE: f64 = 0.5;
/// How far a pixel may be from the sky gradient and still be the gradient, as
/// a sum over its channels.
const GRADIENT_SLACK: i32 = 24;

/// The middle of the picture, where the framed wall is.
fn middle(w: usize, h: usize) -> (std::ops::Range<usize>, std::ops::Range<usize>) {
    (w * 3 / 10..w * 7 / 10, h * 3 / 10..h * 7 / 10)
}

/// The gradient the renderer clears to, at screen row `y` of `h`.
fn gradient(y: usize, h: usize) -> [i32; 3] {
    let cfg = BuildConfig::default();
    let at = |c: u32, shift: u32| i32::from((c >> shift) as u8);
    let t = (y * 256 / h.saturating_sub(1).max(1)).min(256) as i32;
    [16, 8, 0].map(|shift| {
        let (a, b) = (at(cfg.sky_top, shift), at(cfg.sky_bottom, shift));
        a + (b - a) * t / 256
    })
}

/// The share of the middle that is not the gradient, from 0 to 1.
fn not_gradient(p: &Pixels) -> f64 {
    let (w, h, bytes) = (p.0 as usize, p.1 as usize, &p.2);
    let (xs, ys) = middle(w, h);
    let (mut other, mut n) = (0u32, 0u32);
    for y in ys {
        let want = gradient(y, h);
        for x in xs.clone() {
            let i = (y * w + x) * 4;
            let off: i32 = (0..3)
                .map(|c| (i32::from(bytes[i + c]) - want[c]).abs())
                .sum();
            other += u32::from(off > GRADIENT_SLACK);
            n += 1;
        }
    }
    f64::from(other) / f64::from(n.max(1))
}

/// Mean absolute byte difference over the middle; `None` if the sizes differ.
fn middle_diff(a: &Pixels, b: &Pixels) -> Option<f64> {
    if (a.0, a.1) != (b.0, b.1) || a.2.len() != b.2.len() {
        return None;
    }
    let (w, h) = (a.0 as usize, a.1 as usize);
    let (xs, ys) = middle(w, h);
    let (mut sum, mut n) = (0u64, 0u64);
    for y in ys {
        for x in xs.clone() {
            let i = (y * w + x) * 4;
            for c in 0..3 {
                sum += u64::from(a.2[i + c].abs_diff(b.2[i + c]));
                n += 1;
            }
        }
    }
    Some(sum as f64 / n.max(1) as f64)
}

/// The editor with nothing picked or selected and the Select tool, so no
/// highlight is in the picture, once the picture is current and shown.
fn neutral(stage: &Stage<'_, Msg>, probe: &Probe) -> Option<Pixels> {
    {
        let state = probe.state();
        let mut st = state.borrow_mut();
        st.faces.clear();
        st.selection = None;
    }
    stage.emit(Msg::Tool(0));
    let p = wait_for_preview(stage, probe);
    if p != Preview::Full {
        println!("note: the preview ended as {p:?}");
    }
    pump(stage, 25);
    probe.view_pane().frame_pixels()
}

fn layer_slot(st: &EditorState) -> Option<u8> {
    st.doc.entities().iter().find_map(|e| match e.entity {
        Entity::VistaLayer { slot, .. } => Some(slot),
        _ => None,
    })
}

fn pick_middle(stage: &Stage<'_, Msg>, probe: &Probe) {
    let pane = probe.view_pane();
    let (w, h) = pane.size();
    pane.push_click(Click {
        nx: 0.5,
        ny: 0.5,
        aspect: w as f32 / h.max(1) as f32,
        right: false,
        ctrl: false,
        alt: false,
    });
    stage.emit(Msg::Step);
}

/// Builds the vista, paints the wall and reads the picture, Play and the undo.
pub(super) fn script(run: &mut Run, stage: &Stage<'_, Msg>, probe: &Probe) {
    let file = run.dir.join("vista.msl");
    let level = level_text::from_text(LEVEL).expect("the level text");
    std::fs::write(&file, mscent_world::format::write(&level)).expect("writes");
    stage.emit(Msg::OpenPath(file));
    wait_for_preview(stage, probe);
    let state = probe.state();
    let wall = frame_wall(run, stage, probe, |n: Vec3d| n.x > 0.9);
    let Some(wall) = wall else { return };
    let Some(start) = neutral(stage, probe) else {
        run.check(false, "the room is on screen");
        return;
    };
    run.shot(stage, "vista-01-before.png");
    let depth = state.borrow().history.depth();

    // New Vista: one command, one undo step, the layer selected.
    stage.emit(Msg::Menu(cmd::NEW_VISTA));
    let slot = layer_slot(&state.borrow());
    run.check(
        slot == Some(0) && state.borrow().history.depth() == depth + 1,
        &format!("New Vista made a layer in one step: {}", probe.message()),
    );
    run.check(
        matches!(state.borrow().selection, Some(Selection::Entity(_))),
        "the new layer is selected",
    );
    let Some(view) = slot.and_then(view_material) else {
        return;
    };

    // Paint the wall View N as the panel does, after picking it with a click.
    stage.emit(Msg::Tool(FACE_TOOL));
    pick_middle(stage, probe);
    run.check(
        state.borrow().faces.first() == Some(&wall),
        "a click on the picture picked the wall",
    );
    stage.emit(Msg::Face(FaceMsg::Edit(FaceEdit::Material(view))));
    let painted = state.borrow().face_texinfo(wall).map(|t| t.material);
    run.check(painted == Some(view), "the wall holds the View material");
    let Some(sky) = neutral(stage, probe) else {
        return;
    };
    run.shot(stage, "vista-02-sky.png");
    let seen = not_gradient(&sky);
    let moved = middle_diff(&start, &sky);
    run.check(
        seen > 0.5 && moved.is_none_or(|d| d >= MIN_CHANGE),
        &format!("the wall shows the sky: {seen:.2} not gradient, change {moved:?}"),
    );
    still_clickable(run, stage, probe, wall, view);

    // A slot with no layer shows the gradient, and moving the layer back undoes it.
    let before_slot = state.borrow().history.depth();
    let layer = state
        .borrow()
        .doc
        .entities()
        .iter()
        .map(|e| e.id)
        .next_back();
    state.borrow_mut().selection = layer.map(Selection::Entity);
    stage.emit(Msg::Prop(Prop::Param(0, 1.0)));
    let bare = neutral(stage, probe).map(|p| not_gradient(&p));
    run.shot(stage, "vista-03-no-layer.png");
    run.check(
        bare.is_some_and(|s| s < 0.05),
        &format!("a slot with no layer shows the gradient: {bare:?}"),
    );
    while state.borrow().history.depth() > before_slot {
        stage.emit(Msg::Menu(cmd::UNDO));
    }
    let back = neutral(stage, probe).map(|p| not_gradient(&p));
    run.check(
        back.is_some_and(|s| s > 0.5),
        "the layer back, the sky returns",
    );

    play(run, stage, probe);

    // Undo every step: the level and the picture are as they were.
    while state.borrow().history.depth() > depth {
        stage.emit(Msg::Menu(cmd::UNDO));
    }
    let Some(end) = neutral(stage, probe) else {
        return;
    };
    run.shot(stage, "vista-05-undone.png");
    run.check(
        state.borrow().doc.entities().len() == level.entities.len(),
        "undo removed the layer",
    );
    let diff = middle_diff(&start, &end);
    run.check(
        diff.is_none_or(|d| d <= UNDO_TOLERANCE) && (start.0, start.1) == (end.0, end.1),
        &format!("after undo the picture equals the one before (mean diff {diff:?})"),
    );
    if (start.0, start.1) == (end.0, end.1) {
        run.check(start.2 == end.2, "after undo every pixel is the same");
    }
}

/// The painted wall is still found by a click, and painting it back restores it.
fn still_clickable(run: &mut Run, stage: &Stage<'_, Msg>, probe: &Probe, wall: FaceRef, view: u16) {
    let state = probe.state();
    stage.emit(Msg::Tool(FACE_TOOL));
    pick_middle(stage, probe);
    run.check(
        state.borrow().faces.first() == Some(&wall),
        "a wall showing the sky is still picked by a click",
    );
    stage.emit(Msg::Face(FaceMsg::Edit(FaceEdit::Material(0))));
    let back = state.borrow().face_texinfo(wall).map(|t| t.material);
    run.check(back == Some(0), "the face is painted back from the view");
    stage.emit(Msg::Face(FaceMsg::Edit(FaceEdit::Material(view))));
    let again = state.borrow().face_texinfo(wall).map(|t| t.material);
    run.check(again == Some(view), "and to the view again");
    neutral(stage, probe);
}

/// Plays: the player faces the painted wall, so the sky is in the middle.
fn play(run: &mut Run, stage: &Stage<'_, Msg>, probe: &Probe) {
    stage.emit(Msg::Menu(cmd::PLAY));
    let m = wait_for_message(stage, probe, "Playing");
    run.check(m.starts_with("Playing"), &format!("playing the vista: {m}"));
    pump(stage, 40);
    let shown = probe.play_pane().frame_pixels();
    run.shot(stage, "vista-04-play.png");
    let seen = shown.as_ref().map(not_gradient);
    run.check(
        seen.is_some_and(|s| s > 0.5),
        &format!("Play shows the sky through the wall: {seen:?} not gradient"),
    );
    stage.emit(Msg::Escape);
    pump(stage, 10);
}
