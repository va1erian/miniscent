//! The Texture Studio on a stock recipe, which is what people open it on: brick
//! has thirty rows and many blocks that read a name, so the page, its labels and
//! its thumbnails are looked at here as they are, not on a page of four blocks.

use xui::xui_canvas::snapshot::Stage;

use super::texture::settle;
use super::{Run, pump};
use crate::texstudio::Tab;
use crate::ui::texstudio::TexMsg;
use crate::ui::{Msg, Probe};

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
