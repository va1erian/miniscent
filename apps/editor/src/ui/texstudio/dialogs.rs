//! The studio's two questions, as modal windows like the level settings dialog.

use mscent_render::manifest::name_is_valid;
use xui::Dip;
use xui::backend::PlatformSpec;
use xui::prelude::*;
use xui::widget::HasText;

use crate::ui::Msg;

/// The sizes a new material can have. Bigger than the first two is rarely needed,
/// and a size is for good: it sets the material's scale in every level.
const SIDES: [u32; 5] = [64, 128, 256, 512, 1024];
const SIDE_NAMES: [&str; 5] = [
    "64 x 64",
    "128 x 128",
    "256 x 256",
    "512 x 512",
    "1024 x 1024",
];
/// Which one starts chosen: what the stock materials use.
const DEFAULT_SIDE: usize = 1;

#[derive(Clone)]
enum NewMsg {
    Ok,
    Cancel,
}

struct NewDialog {
    name: Handle<Edit<NewMsg>>,
    side: Handle<ComboBox<NewMsg>>,
    problem: Handle<Label<NewMsg>>,
}

impl App for NewDialog {
    type Msg = NewMsg;

    fn update(&mut self, msg: NewMsg, ui: &mut Ui<NewMsg>) {
        match msg {
            NewMsg::Ok => {
                let name = self.name.get().text();
                // Stay open on a bad name so what was typed is not lost.
                if name_is_valid(&name) {
                    let side = SIDES.get(self.side.get().selected()).copied();
                    ui.close_with_result((name, side.unwrap_or(SIDES[DEFAULT_SIDE])));
                } else {
                    self.problem
                        .get()
                        .set_text("Use a-z, 0-9 and -, with a letter or digit at each end.");
                }
            }
            NewMsg::Cancel => ui.close(),
        }
    }
}

/// Asks for a new material's name and size; `None` if cancelled.
pub fn ask_new(ui: &Ui<Msg>) -> Option<(String, u32)> {
    let spec = PlatformSpec::new("New material").size(Dip(380.0), Dip(190.0));
    ui.open_modal(spec, |ui| {
        let dialog = NewDialog {
            name: Handle::new(),
            side: Handle::new(),
            problem: Handle::new(),
        };
        let fields = grid([Track::Auto, Track::Fill(1)])
            .gap(6)
            .child(label("Name"))
            .child(edit().bind(&dialog.name))
            .child(label("Size"))
            .child(combo_box(&SIDE_NAMES).bind(&dialog.side));
        let buttons = row().gap(6).children((
            button("OK").on_click(NewMsg::Ok),
            button("Cancel").on_click(NewMsg::Cancel),
        ));
        let root = column().padding(12).gap(10).children((
            fields,
            label("The size sets its scale in every level, so it cannot change later.").caption(),
            label("").bind(&dialog.problem),
            buttons,
        ));
        if ui.root(root).is_err() {
            ui.close();
        }
        dialog.side.get().select(DEFAULT_SIDE);
        dialog
    })
}

#[derive(Clone)]
enum ConfirmMsg {
    Yes,
    No,
}

struct Confirm;

impl App for Confirm {
    type Msg = ConfirmMsg;

    fn update(&mut self, msg: ConfirmMsg, ui: &mut Ui<ConfirmMsg>) {
        match msg {
            ConfirmMsg::Yes => ui.close_with_result(true),
            ConfirmMsg::No => ui.close(),
        }
    }
}

/// Asks before a stock material is replaced on disk.
pub fn ask_overwrite(ui: &Ui<Msg>, name: &str) -> bool {
    let text = format!("{name} is a stock material. Replace it on disk with this recipe?");
    let spec = PlatformSpec::new("Replace stock material").size(Dip(420.0), Dip(130.0));
    let answer = ui.open_modal(spec, move |ui| {
        let buttons = row().gap(6).children((
            button("Replace").on_click(ConfirmMsg::Yes),
            button("Cancel").on_click(ConfirmMsg::No),
        ));
        let root = column()
            .padding(12)
            .gap(10)
            .children((label(text), buttons));
        if ui.root(root).is_err() {
            ui.close();
        }
        Confirm
    });
    answer.unwrap_or(false)
}
