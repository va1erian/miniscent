//! The Texture Studio tutorial, followed step by step with the messages its clicks
//! send, ending at the page of the tutorial's finished recipe (apart from the
//! crack, which is drawn by hand). Then the last step: Apply, the level using the
//! material, and Save in the order the tutorial says. What the tutorial tells a
//! person to do is what the workspace does, or this fails.

use std::path::Path;

use mscent_render::MaterialTable;
use mscent_texgen::ops::{ParamKind, Value};
use mscent_texgen::recipe;
use xui::xui_canvas::snapshot::Stage;

use super::texture::{add, copy_of_assets, settle, tex};
use super::{Run, select_biggest};
use crate::texstudio::{Stroking, Tab};
use crate::ui::texstudio::{BrushEdit, TexMsg};
use crate::ui::{Msg, Probe};

/// A value as the panel would give it, before the parameter's kind is known.
#[derive(Clone, Copy)]
enum V {
    Int(i32),
    Float(f32),
    Colour([f32; 3]),
    Text(&'static str),
    /// A choice, by the name of the option.
    Mode(&'static str),
}

/// The index of the block at a cell of the open page.
fn block_at(probe: &Probe, (x, y): (u16, u16)) -> Option<usize> {
    let studio = probe.tex();
    let studio = studio.borrow();
    let blocks = &studio.doc()?.recipe().blocks;
    blocks.iter().position(|b| (b.x, b.y) == (x, y))
}

/// Sets a parameter of the block at a cell, by its name, as the panel does.
fn set(run: &mut Run, stage: &Stage<'_, Msg>, probe: &Probe, at: (u16, u16), name: &str, v: V) {
    let found = block_at(probe, at).and_then(|block| {
        let studio = probe.tex();
        let studio = studio.borrow();
        let b = studio.doc()?.recipe().blocks.get(block)?;
        let index = b.spec.params.iter().position(|p| p.name == name)?;
        let value = match (v, &b.spec.params[index].kind) {
            (V::Int(i), ParamKind::Int { .. }) => Value::Int(i),
            (V::Float(f), ParamKind::Float { .. }) => Value::Float(f),
            (V::Colour(c), ParamKind::Color { .. }) => Value::Color([c[0], c[1], c[2], 1.0]),
            (V::Text(t), ParamKind::Text { .. }) => Value::Text(t.to_string()),
            (V::Mode(m), ParamKind::Choice { names, .. }) => {
                Value::Choice(u8::try_from(names.iter().position(|n| *n == m)?).ok()?)
            }
            _ => return None,
        };
        Some((block, index, value))
    });
    match found {
        Some((block, index, value)) => tex(
            stage,
            TexMsg::Param {
                block,
                index,
                value,
                live: false,
            },
        ),
        None => run.check(false, &format!("{name} can be set on the block at {at:?}")),
    }
}

/// Drags the block at a cell to another, as a drag on the page does.
fn drag_to(run: &mut Run, stage: &Stage<'_, Msg>, probe: &Probe, from: (u16, u16), to: (u16, u16)) {
    match block_at(probe, from) {
        Some(block) => tex(
            stage,
            TexMsg::Move {
                block,
                x: to.0,
                y: to.1,
            },
        ),
        None => run.check(false, &format!("a block to drag at {from:?}")),
    }
}

fn widen(run: &mut Run, stage: &Stage<'_, Msg>, probe: &Probe, at: (u16, u16), w: u16) {
    match block_at(probe, at) {
        Some(block) => tex(stage, TexMsg::Resize { block, w }),
        None => run.check(false, &format!("a block to widen at {at:?}")),
    }
}

/// The finished recipe of the tutorial page.
fn tutorial_recipe() -> String {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../docs/texture-studio-tutorial.md"
    );
    let page = std::fs::read_to_string(path).unwrap_or_default();
    let page = page.replace("\r\n", "\n");
    let start = page.find("miniscent-recipe 1").unwrap_or(0);
    let length = page[start..].find("```").unwrap_or(0);
    page[start..start + length].to_string()
}

/// The lines of a recipe that are not strokes, in a fixed order: the tutorial
/// lists the blocks as a person might read them, the file as they were made.
fn without_strokes(text: &str) -> Vec<String> {
    let mut lines: Vec<String> = text
        .lines()
        .filter(|l| !l.starts_with("stroke "))
        .map(str::to_string)
        .collect();
    lines.sort();
    lines
}

/// Steps 1 to 6: the stone, the tiles, the merge, the relief and the shine.
fn build_page(run: &mut Run, stage: &Stage<'_, Msg>, probe: &Probe) {
    tex(
        stage,
        TexMsg::Create {
            name: "slate".to_string(),
            side: 128,
        },
    );
    // 2. Delete the flat; noise in its place.
    tex(stage, TexMsg::Select(Some(0)));
    tex(stage, TexMsg::Delete);
    add(stage, "noise", (0, 0));
    for (name, v) in [
        ("freq_x", V::Int(6)),
        ("freq_y", V::Int(6)),
        ("octaves", V::Int(4)),
        ("seed", V::Int(5)),
        ("color0", V::Colour([0.2, 0.24, 0.28])),
        ("color1", V::Colour([0.42, 0.46, 0.5])),
    ] {
        set(run, stage, probe, (0, 0), name, v);
    }
    // 3. Tiles, and the store that lets other blocks read them.
    add(stage, "bricks", (1, 0));
    for (name, v) in [
        ("rows", V::Int(4)),
        ("cols", V::Int(4)),
        ("offset", V::Float(0.0)),
        ("mortar", V::Float(0.08)),
    ] {
        set(run, stage, probe, (1, 0), name, v);
    }
    add(stage, "store", (1, 1));
    set(run, stage, probe, (1, 1), "name", V::Text("tiles"));
    // 4. Combine them: make room, then a merge two columns wide.
    drag_to(run, stage, probe, (0, 1), (0, 3));
    run.check(
        probe.tex().borrow().trouble().1.is_some(),
        "the page shows an error for a moment, as the tutorial says",
    );
    drag_to(run, stage, probe, (0, 0), (0, 1));
    add(stage, "merge", (0, 2));
    widen(run, stage, probe, (0, 2), 2);
    set(run, stage, probe, (0, 2), "mode", V::Mode("multiply"));
    let marks = probe.tex().borrow().doc().and_then(|d| {
        Some(
            d.page()
                .as_ref()
                .ok()?
                .inputs_of(block_at(probe, (0, 2))?)
                .len(),
        )
    });
    run.check(
        marks == Some(2),
        &format!("the merge reads 2 inputs: {marks:?}"),
    );
    run.check(
        probe.tex().borrow().trouble().1.is_none(),
        "the page compiles with the merge in place",
    );
    // 5. Height, then relief.
    add(stage, "load", (3, 0));
    set(run, stage, probe, (3, 0), "name", V::Text("tiles"));
    add(stage, "blur", (3, 1));
    set(run, stage, probe, (3, 1), "radius", V::Float(0.004));
    add(stage, "out-height", (3, 2));
    set(run, stage, probe, (3, 2), "normal_strength", V::Float(4.0));
    // 6. Shine.
    tex(
        stage,
        TexMsg::Gloss {
            value: 60,
            live: false,
        },
    );
    add(stage, "load", (4, 0));
    set(run, stage, probe, (4, 0), "name", V::Text("tiles"));
    add(stage, "out-specular", (4, 1));
}

/// Step 7: the paint block, the merge widened over it, a crack drawn.
fn draw_crack(run: &mut Run, stage: &Stage<'_, Msg>, probe: &Probe) {
    add(stage, "paint", (2, 1));
    set(
        run,
        stage,
        probe,
        (2, 1),
        "background",
        V::Colour([1.0, 1.0, 1.0]),
    );
    widen(run, stage, probe, (0, 2), 3);
    tex(stage, TexMsg::Select(block_at(probe, (2, 1))));
    tex(stage, TexMsg::Tab(Tab::Block));
    tex(stage, TexMsg::Brush(BrushEdit::Radius(0.012)));
    tex(stage, TexMsg::Brush(BrushEdit::Hardness(0.8)));
    tex(stage, TexMsg::Brush(BrushEdit::Color([0.15, 0.15, 0.15])));
    let path = [[0.1, 0.2], [0.35, 0.45], [0.6, 0.4], [1.1, 0.75]];
    tex(stage, TexMsg::Paint(Stroking::Down(path[0])));
    for pair in path.windows(2) {
        for i in 1..=30 {
            let t = i as f32 / 30.0;
            let at = [
                pair[0][0] + (pair[1][0] - pair[0][0]) * t,
                pair[0][1] + (pair[1][1] - pair[0][1]) * t,
            ];
            tex(stage, TexMsg::Paint(Stroking::Move(at)));
        }
    }
    tex(stage, TexMsg::Paint(Stroking::Up(path[3])));
    run.check(settle(stage, probe), "the cracked page came up");
}

/// The page is the tutorial's, apart from the stroke's points.
fn check_against_the_file(run: &mut Run, probe: &Probe) {
    let ours = probe
        .tex()
        .borrow()
        .doc()
        .map(|d| recipe::write(d.recipe()))
        .unwrap_or_default();
    let theirs = tutorial_recipe();
    run.check(
        !theirs.is_empty() && without_strokes(&ours) == without_strokes(&theirs),
        &format!("the steps make the finished recipe:\n{ours}\nagainst\n{theirs}"),
    );
    let header = |text: &str| {
        let line = text.lines().find(|l| l.starts_with("stroke "));
        let words = line.map(|l| l.split(' ').take(4).collect::<Vec<_>>().join(" "));
        words.unwrap_or_default()
    };
    run.check(
        !header(&ours).is_empty() && header(&ours) == header(&theirs),
        &format!("the crack has the tutorial's brush: {}", header(&ours)),
    );
    let on_grid = ours
        .lines()
        .find(|l| l.starts_with("stroke "))
        .is_some_and(|line| {
            line.split(' ').skip(4).all(|p| {
                p.split(',')
                    .all(|c| c.parse::<f32>().is_ok_and(|v| (v * 4096.0).fract() == 0.0))
            })
        });
    run.check(on_grid, "and its points are on the 1/4096 grid");
}

/// Step 8: Apply, use it in the level, Save in the order the tutorial gives.
fn use_it(run: &mut Run, stage: &Stage<'_, Msg>, probe: &Probe, dir: &Path) {
    let before = probe.materials().current().1.map_or(0, |t| t.len());
    tex(stage, TexMsg::Apply);
    settle(stage, probe);
    let applied = probe.message();
    run.check(applied == "Applied slate", &format!("applied: {applied}"));
    let id = probe
        .materials()
        .current()
        .1
        .ok()
        .and_then(|t| t.find("slate"))
        .map(|id| id.0);
    run.check(
        id.map(usize::from) == Some(before),
        &format!("slate is the next id: {id:?}"),
    );
    // A level that uses it saves, with a warning that names it.
    select_biggest(probe);
    if let Some(id) = id {
        probe
            .state()
            .borrow_mut()
            .edit_selected_brush(|b| b.set_material(id));
    }
    let level = dir.with_extension("msl");
    stage.emit(Msg::SavePath(level.clone()));
    let said = probe.message();
    run.check(
        level.is_file() && said.contains("WARNING") && said.contains("slate"),
        &format!("the level saves, with a warning: {said}"),
    );
    // Another material is applied after it; saving that one first is refused.
    if let Err(why) = copy_of_assets(probe, dir) {
        return run.check(false, &format!("a copy of the assets: {why}"));
    }
    let original = probe.materials().dir();
    probe.materials().set_dir(dir.to_path_buf());
    tex(
        stage,
        TexMsg::Create {
            name: "second".to_string(),
            side: 64,
        },
    );
    tex(stage, TexMsg::Save);
    settle(stage, probe);
    let said = probe.message();
    run.check(
        said.starts_with("not saved") && said.contains("save slate first"),
        &format!("Save names what to save first: {said}"),
    );
    run.check(!dir.join("second.mst").exists(), "and wrote nothing");
    tex(stage, TexMsg::Open("slate".to_string()));
    tex(stage, TexMsg::Save);
    settle(stage, probe);
    let said = probe.message();
    run.check(
        said.starts_with(&format!("Saved slate (id {before})")),
        &format!("slate saved: {said}"),
    );
    stage.emit(Msg::SavePath(level.clone()));
    run.check(
        !probe.message().contains("WARNING"),
        "the level no longer warns",
    );
    tex(stage, TexMsg::Open("second".to_string()));
    tex(stage, TexMsg::Save);
    settle(stage, probe);
    let said = probe.message();
    run.check(
        said.starts_with(&format!("Saved second (id {})", before + 1)),
        &format!("then the other: {said}"),
    );
    let on_disk = MaterialTable::load_dir(dir);
    let ids = on_disk.as_ref().ok().map(|t| {
        (
            t.find("slate").map(|i| usize::from(i.0)),
            t.find("second").map(|i| usize::from(i.0)),
        )
    });
    run.check(
        ids == Some((Some(before), Some(before + 1))),
        &format!("the disk gives the ids the editor gave: {ids:?}"),
    );
    if let Some(original) = original {
        probe.materials().set_dir(original);
    }
    let _ = std::fs::remove_dir_all(dir);
    let _ = std::fs::remove_file(level);
}

pub(super) fn script(run: &mut Run, stage: &Stage<'_, Msg>, probe: &Probe) {
    let p = super::wait_for_preview(stage, probe);
    run.check(
        p == crate::studio::Preview::Full,
        &format!("preview by itself: {p:?}"),
    );
    stage.emit(Msg::Menu(crate::ui::cmd::TEX_OPEN));
    run.check(probe.tex().borrow().is_open(), "the workspace opens");
    build_page(run, stage, probe);
    run.check(settle(stage, probe), "the page came up");
    tex(stage, TexMsg::Tab(Tab::Albedo));
    run.check(settle(stage, probe), "the albedo came up");
    run.check(
        probe.tex().borrow().trouble().1.is_none(),
        "the page of the tutorial has no error",
    );
    draw_crack(run, stage, probe);
    check_against_the_file(run, probe);
    let dir = run.dir.join("tutorial-assets");
    use_it(run, stage, probe, &dir);
}
