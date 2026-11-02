//! "Working..." in the preview ends when the work does, including when it ends in
//! no picture.

use std::time::{Duration, Instant};

use mscent_texgen::ops::{self, Value};
use xui::xui_canvas::snapshot::Stage;

use super::TexMsg;
use super::tests::{Handles, with_workspace};
use crate::ui::Msg;

/// Ticks until the studio has done all it will for the page on show.
fn until_settled(stage: &Stage<'_, Msg>, h: &Handles) {
    let end = Instant::now() + Duration::from_secs(60);
    while !h.studio.borrow().settled() {
        stage.emit(Msg::Step);
        assert!(Instant::now() < end, "never settled");
        std::thread::sleep(Duration::from_millis(1));
    }
    stage.emit(Msg::Step);
}

fn add(stage: &Stage<'_, Msg>, name: &str, cell: (u16, u16)) {
    let op = ops::all().position(|o| o.name == name).expect(name);
    stage.emit(Msg::Tex(TexMsg::Add { op, cell }));
}

fn create(stage: &Stage<'_, Msg>, name: &str) {
    stage.emit(Msg::Tex(TexMsg::Create {
        name: name.to_string(),
        side: 16,
    }));
}

#[test]
fn a_page_that_does_not_compile_clears_the_working_text() {
    with_workspace(|stage, h| {
        // A second albedo output: every block is on the page, and it cannot compile.
        add(stage, "flat", (3, 0));
        add(stage, "out-albedo", (3, 1));
        assert!(h.studio.borrow().doc().is_some_and(|d| d.page().is_err()));
        create(stage, "other");
        until_settled(stage, h);
        assert!(
            h.preview.pixels().is_some(),
            "a page that compiles has its picture"
        );
        stage.emit(Msg::Tex(TexMsg::Open("ui-test".to_string())));
        until_settled(stage, h);
        assert_eq!(h.preview.text(), "", "nothing to wait for");
    });
}

#[test]
fn a_page_whose_first_draft_fails_clears_the_working_text() {
    with_workspace(|stage, h| {
        // An odd count compiles and then fails in the kernel.
        add(stage, "checker", (3, 0));
        let block = h
            .studio
            .borrow()
            .doc()
            .expect("open")
            .selected()
            .expect("it");
        stage.emit(Msg::Tex(TexMsg::Param {
            block,
            index: 0,
            value: Value::Int(3),
            live: false,
        }));
        create(stage, "other");
        until_settled(stage, h);
        stage.emit(Msg::Tex(TexMsg::Open("ui-test".to_string())));
        until_settled(stage, h);
        assert!(
            h.studio.borrow().trouble().1.is_some(),
            "the error label says why"
        );
        assert_eq!(h.preview.text(), "");
    });
}
