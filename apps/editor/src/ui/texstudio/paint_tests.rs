//! Painting with real pointer events on the real preview: where the pointer is in
//! the window decides what is drawn, and what is drawn is read from the recipe and
//! from the pixels the preview ends up showing.

use std::time::{Duration, Instant};

use mscent_texgen::ops::{self, Stroke, Value};
use xui::message::MouseButton;
use xui::xui_canvas::snapshot::Stage;

use super::tests::{Handles, mouse_event, with_workspace};
use super::{BrushEdit, TexMsg};
use crate::studio::tick_costs::TickCosts;
use crate::texstudio::Tab;
use crate::ui::Msg;

/// Adds a paint block to the starter page and selects it, with the picture up.
fn on_a_paint_block(stage: &Stage<'_, Msg>, h: &Handles) {
    let op = ops::all().position(|o| o.name == "paint").expect("paint");
    stage.emit(Msg::Tex(TexMsg::Add { op, cell: (3, 0) }));
    settle(stage, h);
    // Painted once, so the preview knows where its picture is.
    let _ = stage.ui().capture();
}

fn settle(stage: &Stage<'_, Msg>, h: &Handles) {
    let end = Instant::now() + Duration::from_secs(60);
    loop {
        stage.emit(Msg::Step);
        if h.studio.borrow().settled() {
            return;
        }
        assert!(Instant::now() < end, "never settled");
        std::thread::sleep(Duration::from_millis(2));
    }
}

fn strokes(h: &Handles) -> Vec<Stroke> {
    let studio = h.studio.borrow();
    let doc = studio.doc().expect("open");
    let block = doc.selected().expect("selected");
    match doc.recipe().blocks[block].params.values().get(1) {
        Some(Value::Strokes(list)) => list.to_vec(),
        _ => panic!("strokes"),
    }
}

/// A pointer event at `u`, `v` tiles from the picture's corner.
fn at(stage: &Stage<'_, Msg>, h: &Handles, kind: u8, (u, v): (f32, f32)) {
    let (x, y) = h.preview.pixel_at(u, v);
    stage.inject(mouse_event(kind, x, y, MouseButton::Left));
}

fn drag(stage: &Stage<'_, Msg>, h: &Handles, from: (f32, f32), to: (f32, f32)) {
    at(stage, h, 0, from);
    for i in 1..=40 {
        let t = i as f32 / 40.0;
        at(
            stage,
            h,
            1,
            (from.0 + (to.0 - from.0) * t, from.1 + (to.1 - from.1) * t),
        );
    }
    at(stage, h, 2, to);
}

fn red(h: &Handles, x: u32, y: u32) -> u8 {
    let (w, _, rgba) = h.preview.pixels().expect("a picture");
    rgba[((y * w + x) * 4) as usize]
}

fn fat_white_brush(stage: &Stage<'_, Msg>) {
    stage.emit(Msg::Tex(TexMsg::Brush(BrushEdit::Radius(0.1))));
    stage.emit(Msg::Tex(TexMsg::Brush(BrushEdit::Hardness(1.0))));
}

#[test]
fn a_drag_on_the_preview_draws_a_stroke_where_the_pointer_went() {
    with_workspace(|stage, h| {
        on_a_paint_block(stage, h);
        fat_white_brush(stage);
        drag(stage, h, (0.25, 0.5), (0.75, 0.5));
        settle(stage, h);
        let list = strokes(h);
        assert_eq!(list.len(), 1, "one drag, one stroke");
        let (first, last) = (list[0].points[0], *list[0].points.last().expect("points"));
        assert!((first[0] - 0.25).abs() < 0.01 && (first[1] - 0.5).abs() < 0.01);
        assert!((last[0] - 0.75).abs() < 0.01 && (last[1] - 0.5).abs() < 0.01);
        assert_eq!(list[0].radius, 0.1, "the brush of the message");
        // The 16 px picture has ink in the middle of the line, and not at its top.
        assert!(red(h, 8, 8) > 200 && red(h, 8, 1) < 50);
        // One undo takes it all.
        stage.emit(Msg::Menu(crate::ui::cmd::UNDO));
        assert!(strokes(h).is_empty());
    });
}

#[test]
fn a_drag_over_the_seam_of_the_tiled_view_continues_on_the_other_side() {
    with_workspace(|stage, h| {
        on_a_paint_block(stage, h);
        fat_white_brush(stage);
        stage.emit(Msg::Tex(TexMsg::Tiled(true)));
        let _ = stage.ui().capture();
        drag(stage, h, (0.9, 0.5), (1.3, 0.5));
        settle(stage, h);
        let list = strokes(h);
        let last = *list[0].points.last().expect("points");
        assert!((last[0] - 1.3).abs() < 0.01, "the path goes on: {last:?}");
        // 16 px: 0.9..1.3 covers x from 14.4 to the edge and from 0 to 4.8.
        assert!(
            red(h, 15, 8) > 200 && red(h, 2, 8) > 200,
            "both sides inked"
        );
        assert!(red(h, 8, 8) < 50, "and not the middle");
    });
}

#[test]
fn a_press_off_the_picture_or_off_the_block_tab_draws_nothing() {
    with_workspace(|stage, h| {
        on_a_paint_block(stage, h);
        drag(stage, h, (-0.3, 0.5), (0.5, 0.5));
        assert!(strokes(h).is_empty(), "the press was beside the picture");
        stage.emit(Msg::Tex(TexMsg::Tab(Tab::Albedo)));
        settle(stage, h);
        let _ = stage.ui().capture();
        drag(stage, h, (0.2, 0.5), (0.8, 0.5));
        assert!(strokes(h).is_empty(), "Albedo shows the finished material");
    });
}

#[test]
fn the_panel_has_a_brush_for_a_paint_block_and_not_for_another() {
    with_workspace(|stage, h| {
        let has_brush = || stage.ui().layout_report().contains("hardness");
        let _ = stage.ui().capture();
        assert!(!has_brush(), "the starter's flat has no brush");
        on_a_paint_block(stage, h);
        let _ = stage.ui().capture();
        assert!(has_brush(), "{}", stage.ui().layout_report());
    });
}

/// What the tick costs while a stroke is drawn on a 1024 px material: drafts at
/// 128 px, and nothing waited for.
#[test]
fn drawing_on_a_large_material_keeps_ticks_cheap() {
    with_workspace(|stage, h| {
        stage.emit(Msg::Tex(TexMsg::Create {
            name: "large".to_string(),
            side: 1024,
        }));
        on_a_paint_block(stage, h);
        stage.emit(Msg::Tex(TexMsg::Brush(BrushEdit::Radius(0.03))));
        let mut costs = TickCosts::default();
        costs.timed(|| at(stage, h, 0, (0.85, 0.5)));
        for i in 1..=400 {
            let t = i as f32 * 0.05;
            let point = (0.5 + 0.35 * t.cos(), 0.5 + 0.35 * t.sin());
            costs.timed(|| {
                at(stage, h, 1, point);
                stage.emit(Msg::Step);
            });
            std::thread::sleep(Duration::from_millis(1));
        }
        at(stage, h, 2, (0.5, 0.85));
        costs.assert_cheap();
        settle(stage, h);
        assert_eq!(strokes(h).len(), 1);
        assert!(h.studio.borrow().trouble().1.is_none());
    });
}
