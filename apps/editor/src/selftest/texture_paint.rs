//! The paint tool in the selftest, by real pointer events: a stroke drawn across
//! the seam of the tiled view, a second one, undo and redo, then saved and read
//! back from the file the game would read, with the pixels compared at each step.

use std::path::Path;

use mscent_texgen::ops::Value;
use mscent_texgen::recipe;
use xui::backend::Event;
use xui::message::{Modifiers, MouseButton};
use xui::xui_canvas::snapshot::Stage;

use super::texture::{add, copy_of_assets, expected_albedo, settle, tex};
use super::{Run, pump};
use crate::texstudio::Tab;
use crate::ui::texstudio::{BrushEdit, TexMsg};
use crate::ui::{Msg, Probe, cmd};

/// A pointer event at `u`, `v` tiles from the picture's corner.
fn pointer(stage: &Stage<'_, Msg>, probe: &Probe, kind: u8, (u, v): (f32, f32)) {
    let (x, y) = probe.tex_pixel_at(u, v);
    let modifiers = Modifiers::NONE;
    let button = MouseButton::Left;
    stage.inject(match kind {
        0 => Event::MouseDown {
            x,
            y,
            button,
            modifiers,
        },
        1 => Event::MouseMove { x, y, modifiers },
        _ => Event::MouseUp {
            x,
            y,
            button,
            modifiers,
        },
    });
}

/// A drag along a bow from `from` to `to`, a move for each step, with a tick
/// between, as a hand and a window would give them.
fn drag(stage: &Stage<'_, Msg>, probe: &Probe, from: (f32, f32), to: (f32, f32), bow: f32) {
    pointer(stage, probe, 0, from);
    for i in 1..=60 {
        let t = i as f32 / 60.0;
        let lift = bow * 4.0 * t * (1.0 - t);
        let at = (
            from.0 + (to.0 - from.0) * t,
            from.1 + (to.1 - from.1) * t + lift,
        );
        pointer(stage, probe, 1, at);
        stage.emit(Msg::Step);
    }
    pointer(stage, probe, 2, to);
}

fn stroke_count(probe: &Probe) -> usize {
    let studio = probe.tex();
    let studio = studio.borrow();
    let blocks = studio.doc().map(|d| &d.recipe().blocks);
    blocks
        .and_then(|b| b.first())
        .and_then(|b| b.params.values().get(1))
        .map_or(0, |v| match v {
            Value::Strokes(s) => s.len(),
            _ => 0,
        })
}

/// What the Block tab shows, as the generator's own bake of the page says it should.
fn shows_the_bake(run: &mut Run, probe: &Probe, what: &str) {
    let shown = probe.tex_pixels().map(|p| p.2);
    run.check(
        shown.is_some() && shown == expected_albedo(probe),
        &format!("{what}: the preview is the page's own albedo"),
    );
}

pub(super) fn script(run: &mut Run, stage: &Stage<'_, Msg>, probe: &Probe, dir: &Path) {
    tex(
        stage,
        TexMsg::Create {
            name: "selftest-paint".to_string(),
            side: 128,
        },
    );
    // Replace the starter page by a paint block over the albedo output.
    for _ in 0..2 {
        tex(stage, TexMsg::Select(Some(0)));
        tex(stage, TexMsg::Delete);
    }
    add(stage, "paint", (0, 0));
    add(stage, "out-albedo", (0, 1));
    tex(stage, TexMsg::Select(Some(0)));
    tex(stage, TexMsg::Tab(Tab::Block));
    tex(stage, TexMsg::Tiled(true));
    tex(stage, TexMsg::Brush(BrushEdit::Radius(0.05)));
    tex(stage, TexMsg::Brush(BrushEdit::Hardness(0.8)));
    tex(stage, TexMsg::Brush(BrushEdit::Color([1.0, 0.55, 0.1])));
    run.check(settle(stage, probe), "the paint page came up");
    let blank = probe.tex_pixels().map(|p| p.2);
    pump(stage, 3);
    // The preview knows where its picture is from the last time it was drawn.
    let _ = stage.ui().capture();
    // Across the seam between the two tiles, and then a second stroke below it.
    drag(stage, probe, (0.75, 0.3), (1.3, 0.45), 0.08);
    tex(stage, TexMsg::Brush(BrushEdit::Color([0.2, 0.7, 1.0])));
    drag(stage, probe, (0.2, 0.75), (0.9, 0.7), -0.1);
    run.check(settle(stage, probe), "the strokes were drawn");
    run.check(stroke_count(probe) == 2, "two drags, two strokes");
    let drawn = probe.tex_pixels().map(|p| p.2);
    run.check(drawn != blank, "the picture shows them");
    shows_the_bake(run, probe, "after painting");
    // The stroke over the seam is on both sides of it: ink in the first columns of
    // the tile and in the last, where the bow of the stroke crosses them.
    let ink_in = |x: usize| {
        let pixels = probe.tex_pixels();
        pixels.is_some_and(|(w, h, rgba)| {
            (0..h as usize).any(|y| {
                let at = (y * w as usize + x) * 4;
                rgba[at] > 150 && rgba[at + 1] > 60 && rgba[at + 2] < 80
            })
        })
    };
    run.check(
        ink_in(1) && ink_in(126) && !ink_in(64),
        "the stroke over the seam continues on the other side",
    );
    run.shot(stage, "texture-06-painted.png");
    // One stroke is one undo step.
    stage.emit(Msg::Menu(cmd::UNDO));
    run.check(settle(stage, probe), "undone");
    run.check(stroke_count(probe) == 1, "undo takes one stroke");
    stage.emit(Msg::Menu(cmd::REDO));
    run.check(settle(stage, probe), "redone");
    run.check(stroke_count(probe) == 2, "redo gives it back");
    run.check(
        probe.tex_pixels().map(|p| p.2) == drawn,
        "and the same pixels",
    );
    save_and_reload(run, stage, probe, dir);
}

/// Saves to a copy of the assets and reads the recipe back as the game would.
fn save_and_reload(run: &mut Run, stage: &Stage<'_, Msg>, probe: &Probe, dir: &Path) {
    if let Err(why) = copy_of_assets(probe, dir) {
        return run.check(false, &format!("a copy of the assets: {why}"));
    }
    let original = probe.materials().dir();
    probe.materials().set_dir(dir.to_path_buf());
    tex(stage, TexMsg::Save);
    settle(stage, probe);
    let saved = probe.message();
    run.check(saved.starts_with("Saved"), &format!("saved: {saved}"));
    let text = std::fs::read_to_string(dir.join("selftest-paint.mst")).unwrap_or_default();
    run.check(
        text.lines().filter(|l| l.starts_with("stroke ")).count() == 2,
        "the recipe file has the two stroke lines",
    );
    let loaded = recipe::read(text.as_bytes());
    run.check(
        loaded.is_ok(),
        &format!("it reads back: {:?}", loaded.as_ref().err()),
    );
    let same = loaded.is_ok_and(|r| {
        probe
            .tex()
            .borrow()
            .doc()
            .is_some_and(|d| d.recipe().blocks == r.blocks)
    });
    run.check(same, "what reads back is the page that was saved");
    // Whatever comes next saves into a copy of the real assets, not this one.
    if let Some(original) = original {
        probe.materials().set_dir(original);
    }
    let _ = std::fs::remove_dir_all(dir);
}
