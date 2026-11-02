//! The studio's two questions, inside the window. xui's modal windows are not run
//! by its canvas backend, the default: `Ui::open_modal` returns at once with no
//! answer. So a new material is a small form under the material list, and replacing
//! a stock material is an in-window `Dialog`.

use std::cell::Cell;
use std::rc::Rc;

use mscent_render::manifest::name_is_valid;
use xui::backend::{Result as XuiResult, WidgetId};
use xui::prelude::*;
use xui::widget::{Dialog, DialogAction, HasText};

use super::msg::TexMsg;
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

/// The form that asks for a new material's name and size.
#[derive(Default)]
pub struct NewForm {
    pub(super) host: Handle<Panel<Msg>>,
    pub(super) name: Handle<Edit<Msg>>,
    side: Handle<ComboBox<Msg>>,
    pub(super) problem: Handle<Label<Msg>>,
    pub(super) create: Handle<Button<Msg>>,
    pub(super) cancel: Handle<Button<Msg>>,
    pub(super) open: Rc<Cell<bool>>,
}

impl NewForm {
    pub fn layout(&self) -> impl IntoEntry<Msg> + use<> {
        let buttons = row().gap(6).children((
            button("Create")
                .primary()
                .on_click(Msg::Tex(TexMsg::NewSubmit))
                .bind(&self.create),
            button("Cancel")
                .on_click(Msg::Tex(TexMsg::NewCancel))
                .bind(&self.cancel),
        ));
        panel(column().gap(6).children((
            label("Name"),
            edit().bind(&self.name),
            label("Size"),
            combo_box(&SIDE_NAMES).bind(&self.side),
            label("The size is for good.").caption(),
            label("").bind(&self.problem),
            buttons,
        )))
        .plain()
        .bind(&self.host)
    }

    pub fn is_open(&self) -> bool {
        self.open.get()
    }

    /// Shows the form empty, with the cursor in the name.
    pub fn open(&self) {
        self.name.get().set_text("");
        self.problem.get().set_text("");
        self.side.get().select(DEFAULT_SIDE);
        self.host.get().set_visible(true);
        self.open.set(true);
        self.name.get().focus();
    }

    pub fn close(&self) {
        self.open.set(false);
        if let Some(host) = self.host.try_get() {
            host.set_visible(false);
        }
    }

    /// The name and size typed, or says what is wrong with the name.
    pub fn answer(&self) -> Option<(String, u32)> {
        let name = self.name.get().text();
        if name_is_valid(&name) {
            let side = SIDES.get(self.side.get().selected()).copied();
            return Some((name, side.unwrap_or(SIDES[DEFAULT_SIDE])));
        }
        self.say("Use only a-z, 0-9 and -");
        None
    }

    /// Stays open with what was typed, and says why it could not be used.
    pub fn say(&self, why: &str) {
        self.problem.get().set_text(why);
    }

    /// Whether `id` is the name field, which takes typed keys.
    pub fn owns(&self, id: WidgetId) -> bool {
        self.name.try_get().is_some_and(|n| n.id() == id)
    }
}

/// The question before a stock material is replaced on disk.
pub fn overwrite_dialog(ui: &Ui<Msg>) -> XuiResult<Dialog<Msg>> {
    Ok(Dialog::confirm(ui, "Replace stock material", "")?
        .accept_label("Replace")
        .on_action(|action| match action {
            DialogAction::Accept(_) => Some(Msg::Tex(TexMsg::ConfirmOverwrite)),
            DialogAction::Cancel => Some(Msg::Tex(TexMsg::CancelOverwrite)),
        }))
}

/// What the question says about `name`.
pub fn overwrite_question(name: &str) -> String {
    format!("{name} is a stock material. Replace it on disk with this recipe?")
}
