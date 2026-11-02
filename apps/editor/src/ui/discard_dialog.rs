//! Asks before script edits are thrown away. Only the script: its text exists
//! nowhere else, and the code editor's undo does not survive a new level.

use xui::Dip;
use xui::backend::PlatformSpec;
use xui::prelude::*;

use super::app::Msg;

#[derive(Clone)]
enum DiscardMsg {
    Discard,
    Keep,
}

struct DiscardDialog;

impl App for DiscardDialog {
    type Msg = DiscardMsg;

    fn update(&mut self, msg: DiscardMsg, ui: &mut Ui<DiscardMsg>) {
        match msg {
            DiscardMsg::Discard => ui.close_with_result(()),
            DiscardMsg::Keep => ui.close(),
        }
    }
}

/// Whether the user is content to lose the script's unsaved edits.
pub fn may_discard(ui: &Ui<Msg>) -> bool {
    let spec = PlatformSpec::new("Unsaved script").size(Dip(380.0), Dip(130.0));
    let answer: Option<()> = ui.open_modal(spec, |ui| {
        let buttons = row().gap(6).children((
            button("Discard them").on_click(DiscardMsg::Discard),
            button("Keep editing").on_click(DiscardMsg::Keep),
        ));
        let root = column()
            .padding(12)
            .gap(12)
            .children((label("The script has edits that are not saved."), buttons));
        if ui.root(root).is_err() {
            ui.close();
        }
        DiscardDialog
    });
    answer.is_some()
}
