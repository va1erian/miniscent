//! The studio's questions through real clicks and keys. Their first version was a
//! modal window, which tests went around, and which the real backend never showed.

use std::cell::Cell;
use std::rc::Rc;

use xui::message::Modifiers;
use xui::widget::HasText;
use xui::xui_canvas::snapshot::Stage;
use xui::{Key, widget::Placeable};

use super::TexMsg;
use super::tests::{Handles, open, press, with_workspace};
use crate::ui::Msg;

/// The buttons of the New form, to click for real.
pub(super) struct NewFormProbe {
    ui: xui::prelude::Ui<Msg>,
    /// The workspace, and the form's layer and card.
    host: xui::backend::WidgetId,
    form: [xui::backend::WidgetId; 2],
    pub(super) new: Rc<xui::prelude::Button<Msg>>,
    pub(super) create: Rc<xui::prelude::Button<Msg>>,
    pub(super) cancel: Rc<xui::prelude::Button<Msg>>,
    pub(super) name: Rc<xui::prelude::Edit<Msg>>,
    pub(super) problem: Rc<xui::prelude::Label<Msg>>,
    pub(super) open: Rc<Cell<bool>>,
    pub(super) question: Option<Rc<xui::widget::Dialog<Msg>>>,
}

impl NewFormProbe {
    pub(super) fn of(ui: &xui::prelude::Ui<Msg>, tex: &super::Workspace) -> Self {
        Self {
            ui: ui.clone(),
            host: tex.host.get().id(),
            form: [tex.new_form.host.get().id(), tex.new_form.card.get().id()],
            new: tex.new_button.get(),
            create: tex.new_form.create.get(),
            cancel: tex.new_form.cancel.get(),
            name: tex.new_form.name.get(),
            problem: tex.new_form.problem.get(),
            open: tex.new_form.open.clone(),
            question: tex.overwrite.clone(),
        }
    }

    /// Where the middle of a widget is in the window, given the panels it is in:
    /// bounds are relative to the parent.
    fn middle_in(
        &self,
        panels: &[xui::backend::WidgetId],
        id: xui::backend::WidgetId,
    ) -> (i32, i32) {
        let b = self.ui.bounds(id);
        let (mut x, mut y) = ((b.left + b.right) / 2, (b.top + b.bottom) / 2);
        for panel in panels {
            let p = self.ui.bounds(*panel);
            (x, y) = (x + p.left, y + p.top);
        }
        (x, y)
    }

    /// Where a button of the form is.
    pub(super) fn middle(&self, id: xui::backend::WidgetId) -> (i32, i32) {
        self.middle_in(&self.form, id)
    }

    /// Clicks a button of the form.
    pub(super) fn click(&self, stage: &Stage<'_, Msg>, id: xui::backend::WidgetId) {
        self.click_at(stage, self.middle(id));
    }

    /// Clicks the New button, which is outside the form.
    pub(super) fn click_new(&self, stage: &Stage<'_, Msg>) {
        self.click_at(stage, self.middle_in(&[self.host], self.new.id()));
    }

    fn click_at(&self, stage: &Stage<'_, Msg>, (x, y): (i32, i32)) {
        stage.inject(super::tests::mouse_event(
            0,
            x,
            y,
            xui::message::MouseButton::Left,
        ));
        stage.inject(super::tests::mouse_event(
            2,
            x,
            y,
            xui::message::MouseButton::Left,
        ));
    }
}

/// The material being edited.
fn current(h: &Handles) -> Option<String> {
    h.studio.borrow().doc().map(|d| d.name().to_string())
}

fn key(stage: &Stage<'_, Msg>, key: Key) {
    press(stage, key, Modifiers::NONE);
}

fn click_new(stage: &Stage<'_, Msg>, h: &Handles) {
    let _ = stage.ui().capture();
    h.new_form.click_new(stage);
    let _ = stage.ui().capture();
}

fn click_create(stage: &Stage<'_, Msg>, h: &Handles) {
    h.new_form.click(stage, h.new_form.create.id());
    let _ = stage.ui().capture();
}

#[test]
fn the_new_button_opens_a_form_and_create_makes_the_material() {
    with_workspace(|stage, h| {
        let form = &h.new_form;
        assert!(!form.open.get(), "it starts closed");
        click_new(stage, h);
        assert!(form.open.get(), "a click on New shows the form");
        let (x, y) = form.middle(form.create.id());
        assert!(x > 0 && y > 0, "the form has room: {x},{y}");
        // It is in the middle of the window, like the other dialog.
        assert!((x - 640).abs() < 220 && (y - 400).abs() < 180, "{x},{y}");

        form.name.set_text("slate-test");
        click_create(stage, h);
        assert_eq!(current(h).as_deref(), Some("slate-test"));
        assert!(!form.open.get(), "the form goes away");
    });
}

/// A dialog must take the clicks over what is behind it, and the material list,
/// made again as it changes, must not come back above it.
#[test]
fn the_form_stays_above_a_material_list_made_again_while_it_is_open() {
    with_workspace(|stage, h| {
        let form = &h.new_form;
        click_new(stage, h);
        stage.emit(Msg::Tex(TexMsg::Create {
            name: "list-changed".to_string(),
            side: 16,
        }));
        stage.emit(Msg::Step);
        let _ = stage.ui().capture();
        form.name.set_text("after-the-list");
        click_create(stage, h);
        assert_eq!(current(h).as_deref(), Some("after-the-list"));
    });
}

#[test]
fn a_click_behind_the_form_does_nothing() {
    with_workspace(|stage, h| {
        let form = &h.new_form;
        click_new(stage, h);
        let before = current(h);
        // The New button again, under the scrim.
        form.click_new(stage);
        assert!(form.open.get());
        form.name.set_text("still-typed");
        form.click_new(stage);
        assert_eq!(form.name.text(), "still-typed", "New did not start over");
        assert_eq!(current(h), before);
    });
}

#[test]
fn enter_in_the_form_creates_and_esc_only_closes_the_form() {
    with_workspace(|stage, h| {
        let form = &h.new_form;
        click_new(stage, h);
        form.name.set_text("by-enter");
        key(stage, Key::RETURN);
        assert_eq!(current(h).as_deref(), Some("by-enter"));
        assert!(!form.open.get());

        click_new(stage, h);
        key(stage, Key::ESCAPE);
        assert!(!form.open.get());
        assert!(h.studio.borrow().is_open(), "the workspace stayed up");
        assert_eq!(current(h).as_deref(), Some("by-enter"));
    });
}

#[test]
fn a_bad_name_stays_in_the_form_with_the_reason() {
    with_workspace(|stage, h| {
        let form = &h.new_form;
        let before = current(h);
        click_new(stage, h);
        form.name.set_text("Not A Name");
        click_create(stage, h);
        assert_eq!(current(h), before, "nothing was made");
        assert!(form.open.get(), "what was typed is not lost");
        assert_eq!(form.name.text(), "Not A Name");
        assert!(!form.problem.text().is_empty());

        form.name.set_text("ui-test");
        click_create(stage, h);
        assert!(form.open.get(), "a name in use is refused");

        h.new_form.click(stage, form.cancel.id());
        assert!(!form.open.get());
        assert_eq!(current(h), before);
    });
}

#[test]
fn keys_typed_in_the_form_reach_no_shortcut() {
    with_workspace(|stage, h| {
        let blocks = || h.studio.borrow().doc().map(|d| d.recipe().blocks.len());
        let before = blocks();
        click_new(stage, h);
        // Delete would remove a block if it got past the field.
        key(stage, Key::DELETE);
        assert_eq!(blocks(), before);
    });
}

#[test]
fn saving_a_stock_material_asks_in_the_window_and_esc_leaves_it_alone() {
    with_workspace(|stage, h| {
        let question = h.new_form.question.as_ref().expect("made");
        open(stage, h, "brick");
        stage.emit(Msg::Tex(TexMsg::Save));
        assert!(question.is_open(), "the question showed");
        // This test's table is the real assets, so it must never accept.
        key(stage, Key::ESCAPE);
        assert!(!question.is_open());
        assert!(h.studio.borrow().is_open(), "Esc answered, nothing else");
    });
}
