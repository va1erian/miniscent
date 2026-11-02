//! The studio's questions through real clicks and keys. Their first version was a
//! modal window, which tests went around, and which the real backend never showed.

use xui::message::Modifiers;
use xui::widget::HasText;
use xui::xui_canvas::snapshot::Stage;
use xui::{Key, widget::Placeable};

use super::TexMsg;
use super::tests::{Handles, open, press, with_workspace};
use crate::ui::Msg;

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

        form.name.set_text("slate-test");
        click_create(stage, h);
        assert_eq!(current(h).as_deref(), Some("slate-test"));
        assert!(!form.open.get(), "the form goes away");
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
