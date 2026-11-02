//! The studio's two questions, inside the window. xui's modal windows are not run
//! by its canvas backend, the default: `Ui::open_modal` returns at once with no
//! answer. So both are drawn in the window: replacing a stock material is xui's
//! `Dialog`, and a new material a form of ours that looks the same, since it needs a
//! size as well as a name.

use std::cell::Cell;
use std::rc::Rc;

use mscent_render::manifest::name_is_valid;
use xui::backend::{NodeKind, NodeSpec, Result as XuiResult, WidgetId};
use xui::geometry::{Rect, Size};
use xui::layout::Constraints;
use xui::prelude::*;
use xui::widget::{Control, Dialog, DialogAction, HasText, Placeable};

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
/// The form's card and its buttons, in dip, as the window's `Dialog` has them.
const CARD_WIDTH: i32 = 420;
const BUTTON_WIDTH: i32 = 88;

/// Darkens what is behind the form, and takes the clicks meant for it.
struct Scrim {
    control: Control<Msg>,
}

impl Scrim {
    fn new(ui: &Ui<Msg>) -> XuiResult<Scrim> {
        let control = Control::new(
            ui,
            &NodeSpec::new(NodeKind::Custom, Rect::new(0, 0, 10, 10)),
        )?;
        let theme = ui.theme_handle();
        control.set_painter(Rc::new(move |canvas| {
            canvas.fill_rect_rgba(canvas.bounds(), theme.get().scrim);
        }));
        Ok(Scrim { control })
    }
}

impl Placeable<Msg> for Scrim {
    fn id(&self) -> WidgetId {
        self.control.id()
    }

    fn measure(&self, _ui: &Ui<Msg>, _constraints: Constraints) -> Size {
        Size::new(0, 0)
    }
}

/// The form that asks for a new material's name and size.
#[derive(Default)]
pub struct NewForm {
    pub(super) host: Handle<Panel<Msg>>,
    pub(super) card: Handle<Panel<Msg>>,
    pub(super) name: Handle<Edit<Msg>>,
    side: Handle<ComboBox<Msg>>,
    pub(super) problem: Handle<Label<Msg>>,
    pub(super) create: Handle<Button<Msg>>,
    pub(super) cancel: Handle<Button<Msg>>,
    pub(super) open: Rc<Cell<bool>>,
    scrim: Handle<Scrim>,
    /// The form's fixed text, which goes on top with the rest of it.
    labels: [Handle<Label<Msg>>; 4],
    ui: Option<Ui<Msg>>,
}

impl NewForm {
    /// A translucent layer over the whole window with the form in its middle, like
    /// the window's `Dialog`, which holds one text field at most. It belongs above
    /// the workspace in the window's stack.
    pub fn layout(&self) -> impl IntoEntry<Msg> + use<> {
        let buttons = row().gap(8).children((
            spacer(),
            button("Cancel")
                .on_click(Msg::Tex(TexMsg::NewCancel))
                .bind(&self.cancel)
                .width(BUTTON_WIDTH),
            button("Create")
                .primary()
                .on_click(Msg::Tex(TexMsg::NewSubmit))
                .bind(&self.create)
                .width(BUTTON_WIDTH),
        ));
        let fields = grid([Track::Auto, Track::Fill(1)])
            .gap(8)
            .child(label("Name").bind(&self.labels[0]))
            .child(edit().bind(&self.name))
            .child(label("Size").bind(&self.labels[1]))
            .child(combo_box(&SIDE_NAMES).bind(&self.side));
        let card = panel(
            column().padding(16).gap(10).children((
                label("New material").title().bind(&self.labels[2]),
                fields,
                label("The size sets its scale in every level, and cannot change.")
                    .bind(&self.labels[3]),
                label("").bind(&self.problem),
                buttons,
            )),
        )
        .bind(&self.card);
        panel(stack().children((
            build(Scrim::new).bind(&self.scrim),
            overlay().child(card.width(CARD_WIDTH)),
        )))
        .plain()
        .bind(&self.host)
    }

    pub fn is_open(&self) -> bool {
        self.open.get()
    }

    pub fn attach(&mut self, ui: &Ui<Msg>) {
        self.ui = Some(ui.clone());
    }

    /// Shows the form empty, with the cursor in the name.
    pub fn open(&self) {
        self.name.get().set_text("");
        self.problem.get().set_text("");
        self.side.get().select(DEFAULT_SIDE);
        self.host.get().set_visible(true);
        self.open.set(true);
        self.keep_on_top();
        self.name.get().focus();
    }

    /// Puts the form above everything, node by node: what is on top, to paint and
    /// to click, goes by a flat order of all nodes, in which a parent is not above
    /// its children. Widgets made later come after it, so the material list, which
    /// is made again as it changes, needs this each time.
    pub fn keep_on_top(&self) {
        let Some(ui) = self.ui.as_ref().filter(|_| self.open.get()) else {
            return;
        };
        let ids = [
            self.host.get().id(),
            self.scrim.get().id(),
            self.card.get().id(),
            self.name.get().id(),
            self.side.get().id(),
            self.problem.get().id(),
            self.create.get().id(),
            self.cancel.get().id(),
        ];
        let labels = self.labels.iter().map(|l| l.get().id());
        for id in ids.into_iter().chain(labels) {
            ui.raise(id);
        }
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
