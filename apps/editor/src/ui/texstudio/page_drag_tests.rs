//! A block held by the pointer while the page changes under it, and Esc during a
//! drag, with real pointer and key events.

use xui::Key;
use xui::message::Modifiers;
use xui::xui_canvas::snapshot::Stage;

use super::TexMsg;
use super::tests::{CTRL, Handles, at_page, blocks, cell, press, with_workspace};
use crate::ui::Msg;
use mscent_texgen::ops;

fn add(stage: &Stage<'_, Msg>, name: &str, at: (u16, u16)) {
    let op = ops::all().position(|o| o.name == name).expect(name);
    stage.emit(Msg::Tex(TexMsg::Add { op, cell: at }));
}

/// Takes the block in a cell and moves the pointer to another, without letting go.
fn hold(stage: &Stage<'_, Msg>, h: &Handles, from: (i32, i32), to: (i32, i32)) {
    let (x, y) = cell(from.0, from.1);
    at_page(stage, h, 0, x, y);
    let (tx, ty) = cell(to.0, to.1);
    at_page(stage, h, 1, tx, ty);
}

fn let_go(stage: &Stage<'_, Msg>, h: &Handles, at: (i32, i32)) {
    let (x, y) = cell(at.0, at.1);
    at_page(stage, h, 2, x, y);
}

#[test]
fn an_undo_during_a_drag_ends_it_instead_of_moving_a_block_that_is_gone() {
    with_workspace(|stage, h| {
        add(stage, "noise", (2, 0));
        let before = blocks(h);
        assert_eq!(before.len(), 3);
        hold(stage, h, (2, 0), (4, 2));
        // The block being held is un-added.
        press(stage, Key::Z, CTRL);
        assert_eq!(blocks(h).len(), 2);
        let (tx, ty) = cell(4, 3);
        at_page(stage, h, 1, tx, ty);
        let_go(stage, h, (4, 3));
        assert_eq!(blocks(h), before[..2], "nothing moved");
        stage.emit(Msg::Step);
        let said = h.state.borrow().message.clone();
        assert!(!said.contains("no block"), "{said}");
        // The next drag is a fresh one.
        hold(stage, h, (0, 1), (3, 3));
        let_go(stage, h, (3, 3));
        assert_eq!(blocks(h)[1], (3, 3, 1));
    });
}

#[test]
fn deleting_another_block_during_a_drag_does_not_move_the_one_that_took_its_place() {
    with_workspace(|stage, h| {
        add(stage, "noise", (2, 0));
        // The output at (0, 1) is held; the first block is deleted from under it.
        hold(stage, h, (0, 1), (3, 3));
        h.studio.borrow_mut().edit(|d| {
            d.select(Some(0));
            Ok(())
        });
        stage.emit(Msg::Tex(TexMsg::Delete));
        let after_delete = blocks(h);
        assert_eq!(after_delete.len(), 2);
        let (tx, ty) = cell(5, 3);
        at_page(stage, h, 1, tx, ty);
        let_go(stage, h, (5, 3));
        assert_eq!(blocks(h), after_delete, "the block at index 1 stayed put");
    });
}

#[test]
fn escape_cancels_a_drag_first_and_a_second_one_goes_back_to_the_level() {
    with_workspace(|stage, h| {
        let before = blocks(h);
        hold(stage, h, (0, 1), (4, 3));
        press(stage, Key::ESCAPE, Modifiers::NONE);
        assert!(h.studio.borrow().is_open(), "the first Esc only cancels");
        let_go(stage, h, (4, 3));
        assert_eq!(blocks(h), before, "the block did not move");
        assert!(!h.studio.borrow().doc().is_some_and(|d| d.can_undo()));
        press(stage, Key::ESCAPE, Modifiers::NONE);
        assert!(!h.studio.borrow().is_open(), "the second goes back");
    });
}
