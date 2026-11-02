//! Selftest of the Texture Studio, driven by messages: builds a recipe, reads its
//! pixels off the preview, changes a parameter, undoes back to identical pixels,
//! applies to the level and saves to a temporary directory. The directory is a copy
//! of what the game would read, so what is loaded back is what the game would load.

use std::path::{Path, PathBuf};
use std::time::Duration;

use mscent_render::{MaterialId, MaterialTable};
use mscent_texgen::{bake, ops};
use xui::xui_canvas::snapshot::Stage;

use super::{Run, pump, texture_paint, texture_stock, wait_for_preview};
use crate::studio::Preview;
use crate::texstudio::Tab;
use crate::ui::texstudio::TexMsg;
use crate::ui::{Msg, Probe, cmd};

/// Emits steps until the studio has caught up; false when it never did.
pub(super) fn settle(stage: &Stage<'_, Msg>, probe: &Probe) -> bool {
    for _ in 0..30_000 {
        stage.emit(Msg::Step);
        let tex = probe.tex();
        let studio = tex.borrow();
        if studio.settled() && !studio.is_baking() {
            return true;
        }
        drop(studio);
        std::thread::sleep(Duration::from_millis(5));
    }
    false
}

pub(super) fn tex(stage: &Stage<'_, Msg>, msg: TexMsg) {
    stage.emit(Msg::Tex(msg));
}

fn op_index(name: &str) -> usize {
    ops::all()
        .position(|o| o.name == name)
        .unwrap_or(usize::MAX)
}

pub(super) fn add(stage: &Stage<'_, Msg>, name: &str, cell: (u16, u16)) {
    tex(
        stage,
        TexMsg::Add {
            op: op_index(name),
            cell,
        },
    );
}

/// A parameter's index on a block, by the kind of value it holds.
fn colour_index(probe: &Probe, block: usize) -> Option<usize> {
    let studio = probe.tex();
    let studio = studio.borrow();
    let spec = studio.doc()?.recipe().blocks.get(block)?.spec;
    spec.params
        .iter()
        .position(|p| matches!(p.kind, ops::ParamKind::Color { .. }))
}

/// The first block of the open page with a colour to set, and that parameter's index.
/// A stock recipe has its colours wherever it needs them, so this reads the table
/// rather than assuming a place on the page.
fn first_colour(probe: &Probe) -> Option<(usize, usize)> {
    let studio = probe.tex();
    let studio = studio.borrow();
    let blocks = &studio.doc()?.recipe().blocks;
    blocks.iter().enumerate().find_map(|(i, b)| {
        let at = b
            .spec
            .params
            .iter()
            .position(|p| matches!(p.kind, ops::ParamKind::Color { .. }))?;
        Some((i, at))
    })
}

/// A choice parameter of a block that has the named option: its index and the
/// option's.
fn choice_of(probe: &Probe, block: usize, option: &str) -> Option<(usize, u8)> {
    let studio = probe.tex();
    let studio = studio.borrow();
    let spec = studio.doc()?.recipe().blocks.get(block)?.spec;
    spec.params
        .iter()
        .enumerate()
        .find_map(|(i, p)| match p.kind {
            ops::ParamKind::Choice { names, .. } => {
                let at = names.iter().position(|n| *n == option)?;
                Some((i, u8::try_from(at).ok()?))
            }
            _ => None,
        })
}

fn set_colour(stage: &Stage<'_, Msg>, block: usize, index: usize, c: [f32; 3]) {
    tex(
        stage,
        TexMsg::Param {
            block,
            index,
            value: ops::Value::Color([c[0], c[1], c[2], 1.0]),
            live: false,
        },
    );
}

/// What the page should show on the Albedo tab, by the generator's own bake.
pub(super) fn expected_albedo(probe: &Probe) -> Option<Vec<u8>> {
    let studio = probe.tex();
    let studio = studio.borrow();
    let recipe = studio.doc()?.recipe();
    let maps = bake(recipe, recipe.size).ok()?;
    let mut out = Vec::with_capacity(maps.albedo.len() * 4);
    for p in maps.albedo {
        let [_, r, g, b] = p.to_be_bytes();
        out.extend_from_slice(&[r, g, b, 255]);
    }
    Some(out)
}

/// A copy of the stock manifest and textures, so saving touches nothing real.
pub(super) fn copy_of_assets(probe: &Probe, to: &Path) -> Result<(), String> {
    let from = probe.materials().dir().ok_or("no texture directory")?;
    let _ = std::fs::remove_dir_all(to);
    std::fs::create_dir_all(to).map_err(|e| e.to_string())?;
    for entry in std::fs::read_dir(&from).map_err(|e| e.to_string())? {
        let path = entry.map_err(|e| e.to_string())?.path();
        if let Some(name) = path.file_name().filter(|_| path.is_file()) {
            std::fs::copy(&path, to.join(name)).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

/// Builds noise times colour for albedo, and noise for height.
fn build_recipe(run: &mut Run, stage: &Stage<'_, Msg>, probe: &Probe) {
    tex(
        stage,
        TexMsg::Create {
            name: "selftest-mat".to_string(),
            side: 256,
        },
    );
    run.check(
        probe.tex().borrow().doc().is_some(),
        "a new material is open",
    );
    // The starter page is a generator over the albedo output. Clear it.
    for _ in 0..2 {
        tex(stage, TexMsg::Select(Some(0)));
        tex(stage, TexMsg::Delete);
    }
    add(stage, "noise", (0, 0));
    add(stage, "flat", (1, 0));
    add(stage, "merge", (0, 1));
    add(stage, "out-albedo", (0, 2));
    add(stage, "noise", (2, 0));
    add(stage, "out-height", (2, 1));
    add(stage, "flat", (3, 0));
    add(stage, "out-specular", (3, 1));
    let problem = probe.tex().borrow().trouble().1;
    run.check(
        problem.is_none(),
        &format!("the page compiles: {problem:?}"),
    );
    // Noise times a colour: block 1 is the colour and block 2 the merge.
    if let Some(index) = colour_index(probe, 1) {
        set_colour(stage, 1, index, [0.7, 0.35, 0.2]);
    }
    if let Some((index, mode)) = choice_of(probe, 2, "multiply") {
        tex(
            stage,
            TexMsg::Param {
                block: 2,
                index,
                value: ops::Value::Choice(mode),
                live: false,
            },
        );
    }
}

fn check_tabs(run: &mut Run, stage: &Stage<'_, Msg>, probe: &Probe) {
    let mut seen = Vec::new();
    for tab in [Tab::Height, Tab::Normal, Tab::Specular, Tab::Lit] {
        tex(stage, TexMsg::Tab(tab));
        let settled = settle(stage, probe);
        let pixels = probe.tex_pixels();
        run.check(
            settled && pixels.is_some(),
            &format!("the {tab:?} tab shows a picture"),
        );
        seen.push(pixels.map(|p| p.2));
    }
    let all_differ = seen
        .iter()
        .enumerate()
        .all(|(i, a)| seen.iter().skip(i + 1).all(|b| a != b));
    run.check(all_differ, "each tab shows its own picture");
    // Lit is up last: the light moves with nothing but ticks.
    let before = probe.tex_pixels();
    let mut moved = false;
    for _ in 0..200 {
        stage.emit(Msg::Step);
        std::thread::sleep(Duration::from_millis(5));
        if probe.tex_pixels() != before {
            moved = true;
            break;
        }
    }
    run.check(moved, "the lit view's light orbits");
}

fn check_pixels(run: &mut Run, stage: &Stage<'_, Msg>, probe: &Probe) {
    tex(stage, TexMsg::Tab(Tab::Albedo));
    let settled = settle(stage, probe);
    let first = probe.tex_pixels();
    let want = expected_albedo(probe);
    run.check(settled, "the first picture arrived");
    run.check(
        first.as_ref().map(|p| &p.2) == want.as_ref(),
        "the preview is exactly the recipe's albedo",
    );
    // Block 1 is the flat colour.
    let Some(index) = colour_index(probe, 1) else {
        return run.check(false, "flat has a colour");
    };
    set_colour(stage, 1, index, [0.9, 0.1, 0.1]);
    let settled = settle(stage, probe);
    let changed = probe.tex_pixels();
    run.check(
        settled && changed != first,
        "a parameter change reaches the preview",
    );
    let want = expected_albedo(probe);
    run.check(
        changed.as_ref().map(|p| &p.2) == want.as_ref(),
        "and it is the new recipe's albedo",
    );
    stage.emit(Msg::Menu(cmd::UNDO));
    let settled = settle(stage, probe);
    run.check(
        settled && probe.tex_pixels() == first,
        "undo gives back identical pixels",
    );
    stage.emit(Msg::Menu(cmd::REDO));
    let settled = settle(stage, probe);
    run.check(settled && probe.tex_pixels() == changed, "redo too");
    stage.emit(Msg::Menu(cmd::UNDO));
    settle(stage, probe);
    check_broken_page(run, stage, probe, &first);
}

/// A page that does not compile says so, and the last good picture stays.
fn check_broken_page(
    run: &mut Run,
    stage: &Stage<'_, Msg>,
    probe: &Probe,
    good: &Option<(u32, u32, Vec<u8>)>,
) {
    tex(
        stage,
        TexMsg::Move {
            block: 2,
            x: 9,
            y: 9,
        },
    );
    settle(stage, probe);
    let trouble = probe.tex().borrow().trouble();
    run.check(
        trouble.1.is_some() && !trouble.0.is_empty(),
        &format!("a broken page says so: {:?}", trouble.1),
    );
    run.check(
        probe.tex_pixels() == *good,
        "the last good picture stays while it is broken",
    );
    stage.emit(Msg::Menu(cmd::UNDO));
    settle(stage, probe);
    run.check(probe.tex().borrow().trouble().1.is_none(), "undo fixes it");
}

/// Applies a loud `brick` to the level: the 3D view changes, the lightmap does not.
fn check_apply(run: &mut Run, stage: &Stage<'_, Msg>, probe: &Probe) {
    let pane = probe.view_pane();
    let (picture, key) = (pane.frame_checksum(), probe.light_key());
    let generation = probe.materials().generation();
    tex(stage, TexMsg::Open("brick".to_string()));
    let Some((block, index)) = first_colour(probe) else {
        return run.check(false, "brick's page has a colour to set");
    };
    set_colour(stage, block, index, [0.0, 1.0, 1.0]);
    tex(
        stage,
        TexMsg::Gloss {
            value: 7,
            live: false,
        },
    );
    tex(stage, TexMsg::Apply);
    settle(stage, probe);
    run.check(
        probe.materials().generation() == generation + 1,
        &format!("apply makes a new generation: {}", probe.message()),
    );
    let (_, table) = probe.materials().current();
    let gloss = table
        .ok()
        .and_then(|t| t.get(MaterialId(0)).map(|m| m.gloss));
    run.check(gloss == Some(7), "the gloss went with it");
    stage.emit(Msg::Menu(cmd::TEX_CLOSE));
    let p = wait_for_preview(stage, probe);
    run.check(p == Preview::Full, &format!("full light kept: {p:?}"));
    pump(stage, 30);
    run.check(
        pane.frame_checksum() != picture,
        "the applied material reached the 3D view",
    );
    run.check(probe.light_key() == key, "the lightmap key did not change");
    run.shot(stage, "texture-02-applied.png");
    stage.emit(Msg::Menu(cmd::TEX_OPEN));
}

/// Saves the new material in a copy of the assets and loads that as the game does.
fn check_save(run: &mut Run, stage: &Stage<'_, Msg>, probe: &Probe, dir: PathBuf) {
    if let Err(why) = copy_of_assets(probe, &dir) {
        return run.check(false, &format!("a copy of the assets: {why}"));
    }
    let original = probe.materials().dir();
    probe.materials().set_dir(dir.clone());
    tex(stage, TexMsg::Open("selftest-mat".to_string()));
    tex(
        stage,
        TexMsg::Gloss {
            value: 60,
            live: false,
        },
    );
    tex(stage, TexMsg::Save);
    settle(stage, probe);
    run.check(
        probe.message().starts_with("Saved selftest-mat"),
        &format!("saved: {}", probe.message()),
    );
    run.check(
        dir.join("selftest-mat.mst").is_file(),
        "the recipe is there",
    );
    let loaded = MaterialTable::load_dir(&dir);
    run.check(
        loaded.is_ok(),
        &format!("it loads: {:?}", loaded.as_ref().err()),
    );
    let Ok(table) = loaded else {
        return;
    };
    let id = table.find("selftest-mat");
    run.check(
        id.is_some_and(|id| usize::from(id.0) + 1 == table.len()),
        &format!("the last id: {id:?}"),
    );
    let (_, here) = probe.materials().current();
    let here = here.ok();
    let material = id.and_then(|id| table.get(id));
    let shown = id.and_then(|id| here.as_ref()?.get(id));
    let same = material.zip(shown).is_some_and(|(a, b)| {
        a.gloss == b.gloss
            && (0..256).all(|y| (0..256).all(|x| a.texture.texel(x, y) == b.texture.texel(x, y)))
    });
    run.check(same, "what loads is what the editor shows");
    run.check(
        probe
            .tex()
            .borrow()
            .rows()
            .iter()
            .any(|r| r.name == "selftest-mat" && !r.dirty),
        "the list says it is saved",
    );
    // A failure is a message, and nothing is lost: something is in the file's way.
    let target = dir.join("selftest-mat.mst");
    let unblocked = std::fs::remove_file(&target).and_then(|()| std::fs::create_dir(&target));
    run.check(unblocked.is_ok(), "the way is blocked");
    tex(
        stage,
        TexMsg::Gloss {
            value: 61,
            live: false,
        },
    );
    tex(stage, TexMsg::Save);
    settle(stage, probe);
    let message = probe.message();
    run.check(
        message.starts_with("not saved"),
        &format!("failure: {message}"),
    );
    let dirty = probe.tex().borrow().doc().is_some_and(|d| d.dirty());
    run.check(dirty, "still to be saved, with the edit");
    // What follows saves into a copy of the real assets, not into this one.
    if let Some(original) = original {
        probe.materials().set_dir(original);
    }
    let _ = std::fs::remove_dir_all(dir);
}

pub(super) fn script(run: &mut Run, stage: &Stage<'_, Msg>, probe: &Probe) {
    let p = wait_for_preview(stage, probe);
    run.check(p == Preview::Full, &format!("preview by itself: {p:?}"));
    let level = probe.state().borrow().doc.clone();
    stage.emit(Msg::Menu(cmd::TEX_OPEN));
    run.check(probe.tex().borrow().is_open(), "the workspace opens");
    texture_stock::script(run, stage, probe);
    texture_stock::questions(run, stage, probe, &run.dir.join("texture-stock-assets"));
    build_recipe(run, stage, probe);
    check_tabs(run, stage, probe);
    check_pixels(run, stage, probe);
    // Something worth looking at: the page, a block's parameters and a picture.
    tex(stage, TexMsg::Select(Some(2)));
    tex(stage, TexMsg::Tab(Tab::Block));
    tex(stage, TexMsg::Tiled(true));
    settle(stage, probe);
    pump(stage, 3);
    run.shot(stage, "texture-01-workspace.png");
    tex(stage, TexMsg::Tiled(false));
    check_apply(run, stage, probe);
    tex(stage, TexMsg::Tab(Tab::Lit));
    settle(stage, probe);
    run.shot(stage, "texture-03-lit.png");
    check_save(run, stage, probe, run.dir.join("texture-assets"));
    let dir = run.dir.join("texture-paint-assets");
    texture_paint::script(run, stage, probe, &dir);
    run.check(
        probe.state().borrow().doc == level,
        "the level was never touched",
    );
}
