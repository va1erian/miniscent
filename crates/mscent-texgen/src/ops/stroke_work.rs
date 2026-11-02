//! How much work brush strokes are, in the two forms that are asked for: the rule
//! on what `paint` accepts, in tile units, which must not depend on the tile's size;
//! and what a block costs at one size, in nanoseconds, which follows the walk in
//! `paint.rs` closely.

use std::ops::RangeInclusive;

use super::Stroke;
use super::cost::Knobs;

/// Caps on a recipe's strokes, which are untrusted.
pub const MAX_STROKES: usize = 4096;
pub const MAX_POINTS: usize = 4096;
/// Points may leave the tile so a stroke can cross an edge, but not by much: a
/// stroke started on a tile and dragged across the two tiles of the editor's
/// tiled view stays inside this.
pub const STROKE_COORD: RangeInclusive<f32> = -2.0..=3.0;
/// Brush radius, as a fraction of the tile's width.
pub const STROKE_RADIUS: RangeInclusive<f32> = 0.0..=0.5;

/// Most work a `paint` may hold, in square tiles swept. A brush of radius 0.2,
/// thinned as the editor thins it, sweeps 0.18 per segment, so this is 100 tiles
/// of drawing with that thick a brush. And this much work on the costliest shape
/// stays under the recipe limit, so a `paint` that reads can always run.
pub const MAX_STROKE_WORK: f64 = 360.0;
/// What each point adds to the sum however small the brush: the setting up of a
/// segment and its smallest walk, about 16 texels of the largest tile.
const POINT_WORK: f64 = 1.0 / 65536.0;
/// The walk reaches a texel beyond the brush and rounds out by about another. In
/// the rule that is counted at the largest tile, the only one at which it counts.
const RULE_MARGIN: f64 = 2.0 / 1024.0;

/// Nanoseconds per texel walked, over both walks of `lay`, on a long stroke through
/// memory that is not in the cache: repeated strokes in one place cost half this.
const VISIT_NS: f64 = 6.0;
/// Nanoseconds per row walked, over both walks.
const ROW_NS: f64 = 8.0;
/// Nanoseconds a point costs beyond its walk.
const POINT_NS: f64 = 20.0;
/// Nanoseconds per texel of `paint` and `scratches` with nothing to draw: their
/// canvas, in and out of floats.
const CANVAS_NS: f64 = 15.0;
/// How much more a scratch costs than a segment of a stroke: each lands away from
/// the last and blends every texel it walks, where a stroke's segments overlap.
const SCATTERED: f64 = 1.25;

/// The area one segment sweeps, in tile units.
fn capsule(length: f64, reach: f64) -> f64 {
    (length + 2.0 * reach) * 2.0 * reach
}

/// A stroke's segments as `(dx, dy)`, in tile units; a lone point has one, of no
/// length, as it does when it is drawn.
fn deltas(stroke: &Stroke) -> impl Iterator<Item = (f64, f64)> + '_ {
    let delta = |p: &[[f32; 2]]| {
        (
            f64::from(p[1][0]) - f64::from(p[0][0]),
            f64::from(p[1][1]) - f64::from(p[0][1]),
        )
    };
    let lone = (stroke.points.len() == 1).then_some((0.0, 0.0));
    stroke.points.windows(2).map(delta).chain(lone)
}

/// Whether one stroke is something `paint` will run, work aside: finite numbers
/// in range and no more points than the cap.
pub fn stroke_fits(s: &Stroke) -> bool {
    STROKE_RADIUS.contains(&s.radius)
        && (0.0..=1.0).contains(&s.hardness)
        && s.color.iter().all(|c| (0.0..=1.0).contains(c))
        && s.points.len() <= MAX_POINTS
        && s.points.iter().flatten().all(|c| STROKE_COORD.contains(c))
}

/// The one rule on strokes, which `paint` and the recipe reader both apply stroke
/// by stroke, so a file that reads also runs: `spent` plus this stroke's work, or
/// `None` if the stroke is not drawable or the sum goes over [`MAX_STROKE_WORK`].
/// Nothing in it knows the tile's size, so neither does the answer.
pub fn charge_stroke(spent: f64, stroke: &Stroke) -> Option<f64> {
    if !stroke_fits(stroke) {
        return None;
    }
    let reach = f64::from(stroke.radius) + RULE_MARGIN;
    let swept: f64 = deltas(stroke)
        .map(|(dx, dy)| capsule((dx * dx + dy * dy).sqrt(), reach))
        .sum();
    let points = (stroke.points.len() + 1) as f64;
    let total = spent + swept + POINT_WORK * points;
    (total <= MAX_STROKE_WORK).then_some(total)
}

/// Nanoseconds to lay one segment of `dx` by `dy` texels within `reach` texels: rows
/// times the span of each, which is the whole length of a segment too flat to leave
/// a row's band, and the part in the band otherwise.
fn walk_ns(dx: f64, dy: f64, reach: f64) -> f64 {
    let (dx, dy, band) = (dx.abs(), dy.abs(), 2.0 * reach);
    let across = if dy > band { dx * band / dy } else { dx };
    let (rows, span) = (dy + band + 1.0, across + band + 2.0);
    rows * (ROW_NS + span * VISIT_NS) + POINT_NS
}

/// Cost per texel of the `paint` operator.
pub(super) fn paint_cost(k: &Knobs) -> f64 {
    let (w, h) = (f64::from(k.size.width()), f64::from(k.size.height()));
    let work = |s: &Stroke| {
        let reach = f64::from(s.radius) * w + 1.0;
        deltas(s)
            .map(|(dx, dy)| walk_ns(dx * w, dy * h, reach))
            .sum::<f64>()
            + POINT_NS
    };
    let total: f64 = k.strokes("strokes").iter().map(work).sum();
    CANVAS_NS + total / k.size.texels() as f64
}

/// Cost per texel of the `scratches` operator: the same work, from its parameters.
/// Scratches point every way, and a diagonal one costs about what their mean does.
pub(super) fn scratches_cost(k: &Knobs) -> f64 {
    let (w, h) = (f64::from(k.size.width()), f64::from(k.size.height()));
    let along = f64::from(k.float("length")) * core::f64::consts::FRAC_1_SQRT_2;
    let reach = f64::from(k.float("width")) / 2.0 * w + 1.0;
    let one = walk_ns(along * w, along * h, reach) * SCATTERED;
    CANVAS_NS + f64::from(k.int("count")) * one / k.size.texels() as f64
}
