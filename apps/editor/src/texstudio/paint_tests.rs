//! The paint tool through the studio: real pointer positions in, the worker's real
//! pictures out.

use std::sync::Arc;

use crate::model::texdoc::STROKE_POINTS;
use mscent_texgen::ops::{MAX_STROKES, Stroke, Value};

use super::tests::{set_page, settle, studio};
use super::*;

const PAGE: &str = "op 0 0 1 paint background=0.1,0.1,0.1\nop 0 1 1 out-albedo\n";

/// A studio on a page with a paint block, selected, on the Block tab.
fn painter(side: u32) -> TexStudio {
    let mut s = studio();
    set_page(&mut s, side, PAGE);
    s.edit(|d| {
        d.select(Some(0));
        Ok(())
    });
    s.set_tab(Tab::Block);
    s
}

fn strokes(s: &TexStudio) -> Vec<Stroke> {
    let doc = s.doc().expect("open");
    match doc.recipe().blocks[0].params.values().get(1) {
        Some(Value::Strokes(list)) => list.to_vec(),
        _ => panic!("strokes"),
    }
}

/// A drag along a horizontal line, `from` to `to` in tiles, at height `v`.
fn drag(s: &mut TexStudio, from: f32, to: f32, v: f32) {
    s.paint(Stroking::Down([from, v]));
    for i in 1..=60 {
        let u = from + (to - from) * i as f32 / 60.0;
        s.paint(Stroking::Move([u, v]));
    }
    s.paint(Stroking::Up([to, v]));
}

fn red_at(p: &Picture, x: u32, y: u32) -> u8 {
    p.rgba[((y * p.width + x) * 4) as usize]
}

#[test]
fn a_drag_draws_only_on_the_block_tab_over_a_block_with_strokes() {
    let mut s = painter(64);
    for tab in [
        Tab::Albedo,
        Tab::Height,
        Tab::Normal,
        Tab::Specular,
        Tab::Lit,
    ] {
        s.set_tab(tab);
        assert!(!s.can_paint(), "{tab:?}");
        drag(&mut s, 0.2, 0.8, 0.5);
    }
    s.set_tab(Tab::Block);
    s.edit(|d| {
        d.select(Some(1));
        Ok(())
    });
    assert!(!s.can_paint(), "out-albedo has no strokes");
    drag(&mut s, 0.2, 0.8, 0.5);
    s.edit(|d| {
        d.select(None);
        Ok(())
    });
    assert!(!s.can_paint(), "nothing selected");
    s.edit(|d| {
        d.select(Some(0));
        Ok(())
    });
    assert!(s.can_paint());
    assert!(strokes(&s).is_empty(), "none of that drew");
    assert!(!s.doc().expect("open").can_undo());
}

#[test]
fn a_stroke_is_one_undo_step_and_undo_gives_back_identical_pixels() {
    let mut s = painter(64);
    let (before, _) = settle(&mut s);
    let before = before.expect("a first picture");
    drag(&mut s, 0.2, 0.8, 0.5);
    let (drawn, _) = settle(&mut s);
    let drawn = drawn.expect("a picture with the stroke");
    assert_eq!(strokes(&s).len(), 1);
    assert!(red_at(&drawn, 32, 32) > 200, "white where it was drawn");
    assert!(red_at(&drawn, 32, 2) < 50, "and not elsewhere");
    assert_ne!(drawn.rgba, before.rgba);
    assert!(s.undo());
    let (undone, _) = settle(&mut s);
    assert_eq!(undone.expect("a picture").rgba, before.rgba);
    assert!(!s.undo(), "one stroke was one step, and nothing else was");
    assert!(s.redo());
    let (again, _) = settle(&mut s);
    assert_eq!(again.expect("a picture").rgba, drawn.rgba);
}

#[test]
fn the_brush_is_the_tools_and_never_a_step_of_the_documents_history() {
    let mut s = painter(64);
    let brush = Brush {
        radius: 0.1,
        hardness: 1.0,
        color: [1.0, 0.0, 0.0],
    };
    s.set_brush(brush);
    let doc = s.doc().expect("open");
    assert!(!doc.can_undo() && !doc.dirty());
    assert_eq!(s.brush(), brush);
    drag(&mut s, 0.3, 0.7, 0.5);
    let drawn = &strokes(&s)[0];
    assert_eq!((drawn.radius, drawn.hardness), (0.1, 1.0));
    assert_eq!(drawn.color, [1.0, 0.0, 0.0, 1.0]);
    // Changing it afterwards changes nothing already drawn, and is no step.
    s.set_brush(Brush::default());
    assert_eq!(strokes(&s)[0].radius, 0.1);
    assert!(s.undo());
    assert!(strokes(&s).is_empty(), "undo took the stroke");
    assert!(!s.undo(), "and the brush was never a step before it");
    assert_eq!(s.brush(), Brush::default(), "undo left the brush alone");
}

#[test]
fn a_brush_out_of_range_is_brought_in() {
    let mut s = painter(64);
    s.set_brush(Brush {
        radius: 9.0,
        hardness: f32::NAN,
        color: [-1.0, 2.0, 0.5],
    });
    assert_eq!(
        s.brush(),
        Brush {
            radius: 0.5,
            hardness: 0.0,
            color: [0.0, 1.0, 0.5]
        }
    );
}

#[test]
fn a_stroke_across_the_edge_of_the_tiled_view_continues_on_the_other_side() {
    let mut s = painter(64);
    s.set_tiled(true);
    s.set_brush(Brush {
        radius: 0.04,
        hardness: 1.0,
        color: [1.0, 1.0, 1.0],
    });
    // Down on the second tile of the view and drag left over the seam between the
    // two tiles, then on to the first: tile positions 0.9 -> 1.3 are one stroke.
    drag(&mut s, 1.9, 2.0, 1.5);
    let first = strokes(&s);
    // Rounded to the grid points sit on: at most half a step of 1/4096 off.
    let press = first[0].points[0];
    let near = |a: f32, b: f32| (a - b).abs() <= 0.5 / 4096.0 + 1e-6;
    assert!(
        near(press[0], 0.9) && near(press[1], 0.5),
        "tile position of the press: {press:?}"
    );
    s.undo();
    drag(&mut s, 0.9, 1.3, 0.5);
    let stroke = &strokes(&s)[0];
    let last = stroke.points.last().expect("points");
    assert!(
        (last[0] - 1.3).abs() < 1e-4,
        "the path does not wrap: {last:?}"
    );
    let (picture, _) = settle(&mut s);
    let p = picture.expect("a picture");
    // Both sides of the edge are inked, and the middle is not.
    assert!(red_at(&p, 60, 32) > 200 && red_at(&p, 10, 32) > 200);
    assert!(red_at(&p, 32, 32) < 50);
}

#[test]
fn a_stroke_stops_at_the_most_points_and_says_so_once_without_failing() {
    let mut s = painter(64);
    s.set_brush(Brush {
        radius: 0.0,
        ..Brush::default()
    });
    s.paint(Stroking::Down([0.5, 0.5]));
    let mut told = 0;
    for i in 0..STROKE_POINTS + 200 {
        let u = if i % 2 == 0 { 0.52 } else { 0.5 };
        s.paint(Stroking::Move([u + i as f32 * 1e-7, 0.5]));
        told += usize::from(
            s.message
                .take()
                .is_some_and(|m| m.contains("stops growing")),
        );
    }
    s.paint(Stroking::Up([0.5, 0.5]));
    assert_eq!(told, 1, "once, not at every move after");
    assert_eq!(strokes(&s)[0].points.len(), STROKE_POINTS);
    let (picture, _) = settle(&mut s);
    assert!(
        picture.is_some() && s.trouble().1.is_none(),
        "{:?}",
        s.trouble()
    );
}

#[test]
fn a_stroke_past_the_most_strokes_is_refused_with_a_message() {
    let mut s = painter(64);
    let dab = Stroke {
        radius: 0.0,
        hardness: 1.0,
        color: [1.0; 4],
        points: vec![[0.5, 0.5]],
    };
    let full = Arc::new(vec![dab; MAX_STROKES]);
    s.edit(|d| d.set_param(0, 1, Value::Strokes(full), false))
        .expect("sets");
    let steps = s.doc().expect("open").can_undo();
    drag(&mut s, 0.2, 0.8, 0.5);
    let message = s.message.take().expect("it says why");
    assert!(message.contains("4096"), "{message}");
    assert_eq!(strokes(&s).len(), MAX_STROKES);
    assert_eq!(s.doc().expect("open").can_undo(), steps);
    s.undo();
    assert_eq!(
        strokes(&s).len(),
        0,
        "the undo was the setting, not a stroke"
    );
}

#[test]
fn while_dragging_the_picture_is_a_draft_and_the_full_one_comes_on_release() {
    let mut s = painter(256);
    settle(&mut s);
    s.paint(Stroking::Down([0.2, 0.5]));
    let mut seen = Vec::new();
    for i in 1..=40 {
        s.paint(Stroking::Move([0.2 + i as f32 * 0.015, 0.5]));
        if let Some(p) = s.tick().picture {
            seen.push((p.width, p.full));
        }
        std::thread::sleep(std::time::Duration::from_millis(3));
    }
    assert!(
        !seen.is_empty(),
        "drafts arrived while the pointer was down"
    );
    assert!(seen.iter().all(|&(w, full)| w == 128 && !full), "{seen:?}");
    s.paint(Stroking::Up([0.8, 0.5]));
    let (picture, _) = settle(&mut s);
    let p = picture.expect("a picture");
    assert!(
        p.full && p.width == 256,
        "full size on release: {}",
        p.width
    );
}

#[test]
fn a_press_that_does_not_move_is_a_dab() {
    let mut s = painter(64);
    s.set_brush(Brush {
        radius: 0.1,
        ..Brush::default()
    });
    s.paint(Stroking::Down([0.5, 0.5]));
    s.paint(Stroking::Up([0.5, 0.5]));
    assert_eq!(strokes(&s)[0].points, [[0.5, 0.5]]);
    let (picture, _) = settle(&mut s);
    assert!(red_at(&picture.expect("a picture"), 32, 32) > 200);
}
