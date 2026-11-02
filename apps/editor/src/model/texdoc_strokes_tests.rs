use mscent_texgen::recipe;

use super::*;

/// A paint block fed to the albedo output.
fn doc() -> TexDoc {
    let recipe = recipe::read(
        b"miniscent-recipe 1\nsize 16 16\nop 0 0 1 paint\nop 0 1 1 out-albedo\nop 1 0 1 flat\n",
    )
    .expect("reads");
    TexDoc::open("p", recipe, 40)
}

fn brush(radius: f32, at: [f32; 2]) -> Stroke {
    Stroke {
        radius,
        hardness: 0.5,
        color: [1.0, 0.0, 0.0, 1.0],
        points: vec![at],
    }
}

fn points(d: &TexDoc) -> Vec<Vec<[f32; 2]>> {
    let Some(ops::Value::Strokes(s)) = d.recipe().blocks[0].params.values().get(1) else {
        panic!("strokes");
    };
    s.iter().map(|s| s.points.clone()).collect()
}

#[test]
fn a_whole_stroke_is_one_undo_step_and_undo_gives_the_page_back() {
    let mut d = doc();
    let before = d.recipe().clone();
    d.begin_stroke(0, brush(0.04, [0.5, 0.5])).expect("begins");
    for i in 1..=40 {
        d.extend_stroke([0.5 + i as f32 * 0.012, 0.5], false);
    }
    d.end_stroke();
    assert_eq!(points(&d)[0].len(), 41);
    assert!(d.dirty() && d.can_undo());
    assert!(d.undo());
    assert_eq!(d.recipe(), &before, "one step undoes the stroke");
    assert!(!d.can_undo());
    assert!(d.redo());
    assert_eq!(points(&d)[0].len(), 41, "and redo draws it again");
}

#[test]
fn points_closer_than_a_quarter_of_the_radius_are_not_kept() {
    let mut d = doc();
    d.begin_stroke(0, brush(0.04, [0.5, 0.5])).expect("begins");
    // 0.04 * 0.25 = 0.01 apart at least.
    let mut kept = 0;
    for i in 1..=1000 {
        let grew = d.extend_stroke([0.5 + i as f32 * 0.0001, 0.5], false);
        kept += usize::from(grew == Growth::Added);
    }
    assert!(
        (9..=10).contains(&kept),
        "a tenth of a tile in steps of a hundredth: {kept}"
    );
    // The end of the stroke is where the pointer was let go, even when near.
    let end = points(&d)[0].last().map_or(0.0, |p| p[0]) + 0.002;
    assert_eq!(d.extend_stroke([end, 0.5], false), Growth::Skipped);
    assert_eq!(d.extend_stroke([end, 0.5], true), Growth::Added);
    assert_eq!(
        d.extend_stroke([end, 0.5], true),
        Growth::Skipped,
        "not twice"
    );
}

#[test]
fn a_hairline_brush_still_has_a_floor_on_its_spacing() {
    let mut d = doc();
    d.begin_stroke(0, brush(0.0, [0.5, 0.5])).expect("begins");
    assert_eq!(d.extend_stroke([0.5, 0.5001], false), Growth::Skipped);
    assert_eq!(d.extend_stroke([0.5, 0.502], false), Growth::Added);
}

#[test]
fn a_stroke_stops_growing_at_the_point_cap_and_stays_valid() {
    let mut d = doc();
    d.begin_stroke(0, brush(0.0, [0.5, 0.5])).expect("begins");
    let mut full = 0;
    for i in 0..MAX_POINTS + 10 {
        let x = if i % 2 == 0 { 0.5 } else { 0.52 };
        full += usize::from(d.extend_stroke([x, 0.5], false) == Growth::Full);
    }
    d.end_stroke();
    assert_eq!(points(&d)[0].len(), MAX_POINTS);
    assert!(full > 0, "the points past it are told so, none is added");
    let Some(ops::Value::Strokes(s)) = d.recipe().blocks[0].params.values().get(1) else {
        panic!("strokes");
    };
    assert!(s.iter().all(ops::stroke_fits), "the operator will run it");
}

#[test]
fn a_block_holds_at_most_the_most_strokes_and_a_refusal_changes_nothing() {
    let mut d = doc();
    let dab = brush(0.0, [0.5, 0.5]);
    for _ in 0..MAX_STROKES {
        d.begin_stroke(0, dab.clone()).expect("room");
        d.end_stroke();
    }
    let before = d.recipe().clone();
    let steps = d.past.len();
    let refused = d.begin_stroke(0, dab);
    assert!(
        refused.as_ref().is_err_and(|m| m.contains("4096")),
        "{refused:?}"
    );
    assert_eq!(d.recipe(), &before);
    assert_eq!(d.past.len(), steps, "no undo step for what did not happen");
    assert_eq!(d.stroke_count(0), MAX_STROKES);
}

#[test]
fn only_a_block_with_strokes_takes_them_and_only_a_drawable_brush() {
    let mut d = doc();
    assert!(d.begin_stroke(2, brush(0.1, [0.0, 0.0])).is_err(), "a flat");
    assert!(
        d.begin_stroke(9, brush(0.1, [0.0, 0.0])).is_err(),
        "no block"
    );
    assert!(
        d.begin_stroke(0, brush(0.9, [0.0, 0.0])).is_err(),
        "too fat"
    );
    assert!(d.begin_stroke(0, brush(0.1, [f32::NAN, 0.0])).is_err());
    assert!(!d.can_undo(), "none of that was an edit");
    assert!(d.can_stroke(0) && !d.can_stroke(1) && !d.can_stroke(2));
}

#[test]
fn growing_with_no_stroke_begun_or_a_point_out_of_range_does_nothing() {
    let mut d = doc();
    assert_eq!(d.extend_stroke([0.5, 0.5], false), Growth::Skipped);
    d.begin_stroke(0, brush(0.0, [0.5, 0.5])).expect("begins");
    for far in [
        [3.5, 0.5],
        [0.5, -2.5],
        [f32::NAN, 0.0],
        [f32::INFINITY, 0.0],
    ] {
        assert_eq!(d.extend_stroke(far, true), Growth::Skipped, "{far:?}");
    }
    assert_eq!(d.extend_stroke([2.9, -1.9], true), Growth::Added);
    d.end_stroke();
    assert_eq!(d.extend_stroke([0.7, 0.5], true), Growth::Skipped, "ended");
}

#[test]
fn undoing_in_the_middle_of_a_stroke_ends_it_and_the_panel_follows_the_count() {
    let mut d = doc();
    d.begin_stroke(0, brush(0.0, [0.5, 0.5])).expect("begins");
    let epoch = d.panel_epoch();
    assert_eq!(d.extend_stroke([0.6, 0.5], false), Growth::Added);
    assert_eq!(d.panel_epoch(), epoch, "growing leaves the panel alone");
    d.undo();
    assert_eq!(d.extend_stroke([0.7, 0.5], false), Growth::Skipped);
    assert_eq!(d.stroke_count(0), 0);
    d.begin_stroke(0, brush(0.0, [0.5, 0.5])).expect("begins");
    let epoch = d.panel_epoch();
    d.end_stroke();
    assert_ne!(d.panel_epoch(), epoch, "the panel counts the strokes");
    let epoch = d.panel_epoch();
    d.clear_strokes(0, 1).expect("clears");
    assert_ne!(d.panel_epoch(), epoch);
    assert_eq!(d.stroke_count(0), 0);
}
