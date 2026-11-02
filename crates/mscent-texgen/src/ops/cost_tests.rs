//! What the limits promise: a cost per operator, a work rule that does not move
//! with the tile's size, and a walk that misses nothing the brush touches.

use std::sync::Arc;

use super::paint::covered_outside_walk;
use super::test_util::{run, square};
use super::{
    MAX_STROKE_WORK, OpSpec, Params, Role, Stroke, Value, all, charge_stroke, cost_ns, find,
};
use crate::graph::{self, MAX_RECIPE_NS};
use crate::image::Size;
use crate::recipe;

fn stroke(radius: f32, points: Vec<[f32; 2]>) -> Stroke {
    Stroke {
        radius,
        hardness: 0.5,
        color: [1.0; 4],
        points,
    }
}

fn work_of(list: &[Stroke]) -> Option<f64> {
    list.iter().try_fold(0.0, charge_stroke)
}

fn paint(list: Vec<Stroke>, size: Size) -> bool {
    run(
        "paint",
        &[("strokes", Value::Strokes(Arc::new(list)))],
        &[],
        size,
    )
    .is_ok()
}

/// A hand's page: points a quarter of the radius apart, on a zigzag that stays on
/// the tiles it covers, as far as `tiles` of drawing.
fn hand_drawn(radius: f32, tiles: f32) -> Stroke {
    let step = radius / 4.0;
    let count = (tiles / step) as usize;
    let points = (0..count)
        .map(|i| {
            let along = (i as f32 * step).rem_euclid(4.0);
            let there_and_back = if along > 2.0 { 4.0 - along } else { along };
            [there_and_back - 0.5, 0.25 + (i % 2) as f32 * step]
        })
        .collect();
    stroke(radius, points)
}

#[test]
fn every_kernel_has_a_cost_and_nothing_else_does() {
    let at = square(1024);
    let cost = |op: &'static OpSpec| cost_ns(op, &Params::defaults(op), 1, at);
    for op in all() {
        if op.role == Role::Kernel {
            assert!(cost(op) > 0, "{}", op.name);
        } else {
            assert_eq!(cost(op), 0, "{}", op.name);
        }
    }
}

#[test]
fn costs_follow_the_parameters_that_matter() {
    let at = square(256);
    let cost = |name, key, value| {
        let op = find(name).expect(name);
        let mut params = Params::defaults(op);
        params.set(op, key, value).expect(key);
        cost_ns(op, &params, 2, at)
    };
    let octaves = |name, n| cost(name, "octaves", Value::Int(n));
    assert!(octaves("noise", 8) > 3 * octaves("noise", 2));
    assert!(octaves("wavelet", 6) > 2 * octaves("wavelet", 1));
    let count = |n| cost("scratches", "count", Value::Int(n));
    assert!(count(512) > 2 * count(1));
    let merge = find("merge").expect("merge");
    let inputs = |n| cost_ns(merge, &Params::defaults(merge), n, at);
    assert!(inputs(8) > 5 * inputs(2));
}

/// Cost is a function of the recipe and the size, so it scales with the texels for
/// an operator that has no strokes and is the same each time it is asked.
#[test]
fn cost_is_deterministic_and_grows_with_the_size() {
    let blur = find("blur").expect("blur");
    let params = Params::defaults(blur);
    let (small, large) = (
        cost_ns(blur, &params, 1, square(128)),
        cost_ns(blur, &params, 1, square(1024)),
    );
    assert_eq!(large, 64 * small);
    assert_eq!(small, cost_ns(blur, &params, 1, square(128)));
}

/// The old rule counted texels, so many points of a real brush failed at 128 px
/// and worked at 1024 px, and left the editor's draft showing an error on a valid
/// page. These are that page, and pages to either side of the new limit.
#[test]
fn strokes_get_the_same_answer_at_128_and_1024_px() {
    let many: Vec<Stroke> = vec![hand_drawn(0.03, 30.0); 10];
    let points: usize = many.iter().map(|s| s.points.len()).sum();
    assert!(points > 30_000, "{points} points");
    let big_dab = stroke(0.5, vec![[0.5, 0.5]]);
    let dab_work = work_of(std::slice::from_ref(&big_dab)).expect("one fits");
    let fitting = (MAX_STROKE_WORK / dab_work) as usize;
    let at_limit = vec![big_dab.clone(); fitting];
    let past_limit = vec![big_dab; fitting + 1];
    for (list, want) in [(many, true), (at_limit, true), (past_limit, false)] {
        assert_eq!(work_of(&list).is_some(), want);
        assert_eq!(paint(list.clone(), square(128)), want);
        assert_eq!(paint(list, square(1024)), want);
    }
}

/// A real page is a long stroke with the thickest brush anyone uses, across many
/// tiles' worth of drawing, a few times over, and it is never refused.
#[test]
fn a_real_hand_drawn_page_is_within_the_rule_and_the_recipe_limit() {
    let page = vec![hand_drawn(0.2, 45.0); 2];
    assert!(work_of(&page).is_some());
    let thin = vec![hand_drawn(0.01, 10.0); 64];
    assert!(work_of(&thin).is_some());
    let op = find("paint").expect("paint");
    let mut params = Params::defaults(op);
    params
        .set(op, "strokes", Value::Strokes(Arc::new(page)))
        .expect("strokes");
    assert!(cost_ns(op, &params, 0, square(1024)) < MAX_RECIPE_NS);
}

/// However much a `paint` is allowed by the rule, one block of it runs: the rule
/// never accepts a page the recipe limit then refuses, on its own.
#[test]
fn the_most_a_paint_may_hold_is_within_the_recipe_limit_on_any_shape() {
    let mut shapes = vec![
        stroke(0.5, vec![[0.5, 0.5]; 2]),
        stroke(0.0, vec![[-2.0, -2.0], [3.0, 3.0]]),
    ];
    for radius in [0.0, 0.001, 0.003, 0.01, 0.03, 0.1, 0.2, 0.3, 0.5] {
        for slope in [0.0, 0.25, 0.5, 1.0, 1.5, 2.0, 4.0, 8.0] {
            let dy = (radius + 2.0 / 1024.0) * 2.0 * slope;
            shapes.push(stroke(radius, vec![[-2.0, 0.5], [3.0, 0.5 + dy.min(2.5)]]));
        }
    }
    for one in shapes {
        let work = work_of(std::slice::from_ref(&one)).expect("one fits");
        let count = ((MAX_STROKE_WORK / work) as usize).min(super::MAX_STROKES);
        let op = find("paint").expect("paint");
        let mut params = Params::defaults(op);
        params
            .set(op, "strokes", Value::Strokes(Arc::new(vec![one; count])))
            .expect("strokes");
        for (w, h) in [(1024, 1024), (1024, 4), (4, 1024), (128, 128)] {
            let size = Size::new(w, h).expect("size");
            let cost = cost_ns(op, &params, 0, size);
            assert!(cost < MAX_RECIPE_NS, "{w}x{h}: {cost} ns");
        }
    }
}

/// The dearest ordinary page anyone could mean, at 1024 px, fits the limit. The
/// limit is there for hostile pages only.
#[test]
fn thirty_heavy_blocks_at_1024_px_fit() {
    let cost_of = |dear: &[&str]| {
        let mut text = String::from("miniscent-recipe 1\nsize 1024 1024\n");
        for i in 0..30 {
            text.push_str(&format!("op {} 0 1 {}\n", i, dear[i % dear.len()]));
        }
        let r = recipe::read(text.as_bytes()).expect("reads");
        let g = graph::compile(&r).expect("compiles");
        graph::page_cost(&r, &g, r.size)
    };
    let mixed = cost_of(&[
        "noise octaves=8",
        "dots count_x=64 count_y=64",
        "scratches count=512 length=0.5 width=0.05",
        "wavelet octaves=6",
    ]);
    assert!(mixed < MAX_RECIPE_NS, "{mixed} ns");
    let dots = cost_of(&["dots count_x=64 count_y=64"]);
    assert!(dots < MAX_RECIPE_NS, "{dots} ns");
}

/// The walk leaves out most of a long segment's bounding box, which must never
/// include a texel the brush would have touched: that would change the picture.
#[test]
fn the_walk_reaches_every_texel_the_brush_covers() {
    let lines: [[[f32; 2]; 2]; 8] = [
        [[10.0, 10.0], [10.0, 10.0]],
        [[10.0, 10.0], [90.0, 10.0]],
        [[10.0, 10.0], [10.0, 90.0]],
        [[10.0, 10.0], [90.0, 90.0]],
        [[90.0, 10.0], [10.0, 90.0]],
        [[10.0, 40.0], [90.0, 44.0]],
        [[-30.0, 20.5], [140.0, 21.5]],
        [[50.25, -20.75], [52.5, 150.125]],
    ];
    for seg in lines {
        for radius in [0.0, 0.4, 1.0, 3.5, 20.0] {
            for hardness in [0.0, 0.5, 1.0] {
                assert_eq!(
                    covered_outside_walk(seg, radius, hardness),
                    0,
                    "{seg:?} radius {radius} hardness {hardness}"
                );
            }
        }
    }
}
