//! The Texture Studio on a stock recipe, which is what people open it on: brick
//! has thirty rows and many blocks that read a name, so the page, its labels and
//! its thumbnails are looked at here as they are, not on a page of four blocks.

use std::path::Path;

use xui::xui_canvas::snapshot::Stage;

use super::texture::{copy_of_assets, settle};
use super::{Run, pump};
use crate::texstudio::Tab;
use crate::ui::texstudio::TexMsg;
use crate::ui::{Msg, Probe, cmd};

/// Opens brick, selects a block and takes a picture of the whole workspace.
pub(super) fn script(run: &mut Run, stage: &Stage<'_, Msg>, probe: &Probe) {
    stage.emit(Msg::Tex(TexMsg::Open("brick".to_string())));
    stage.emit(Msg::Tex(TexMsg::Tab(Tab::Block)));
    stage.emit(Msg::Tex(TexMsg::Select(Some(9))));
    run.check(settle(stage, probe), "brick's page came up");
    pump(stage, 3);
    run.shot(stage, "texture-04-brick.png");
    // The longest recipe, at its last block, which has the longest name.
    stage.emit(Msg::Tex(TexMsg::Open("metal".to_string())));
    let last = probe
        .tex()
        .borrow()
        .doc()
        .map_or(0, |d| d.recipe().blocks.len().saturating_sub(1));
    stage.emit(Msg::Tex(TexMsg::Select(Some(last))));
    run.check(settle(stage, probe), "metal's page came up");
    pump(stage, 3);
    run.shot(stage, "texture-05-metal-end.png");
}

/// The two questions, on screen: the form for a new material, and the one asked
/// before a stock material is replaced, answered in a copy of the assets.
pub(super) fn questions(run: &mut Run, stage: &Stage<'_, Msg>, probe: &Probe, dir: &Path) {
    stage.emit(Msg::Tex(TexMsg::AskNew));
    pump(stage, 3);
    run.shot(stage, "texture-07-new.png");
    stage.emit(Msg::Tex(TexMsg::NewCancel));
    if let Err(why) = copy_of_assets(probe, dir) {
        return run.check(false, &format!("a copy of the assets: {why}"));
    }
    let original = probe.materials().dir();
    probe.materials().set_dir(dir.to_path_buf());
    stage.emit(Msg::Tex(TexMsg::Open("brick".to_string())));
    run.check(settle(stage, probe), "brick is open to replace");
    stage.emit(Msg::Tex(TexMsg::Gloss {
        value: 26,
        live: false,
    }));
    stage.emit(Msg::Tex(TexMsg::Save));
    pump(stage, 3);
    run.shot(stage, "texture-08-replace.png");
    let before = std::fs::read_to_string(dir.join("materials.txt")).unwrap_or_default();
    run.check(
        before.contains("0 brick 25"),
        "nothing written before the answer",
    );
    // Enter, the way a person may answer; it closes the question too.
    stage.emit(Msg::Guarded(Box::new(Msg::Menu(cmd::SLICE))));
    run.check(settle(stage, probe), "the replacement settled");
    let after = std::fs::read_to_string(dir.join("materials.txt")).unwrap_or_default();
    run.check(after.contains("0 brick 26"), "Replace wrote it");
    if let Some(original) = original {
        probe.materials().set_dir(original);
    }
}
