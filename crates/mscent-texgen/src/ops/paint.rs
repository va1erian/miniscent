//! Round-brush strokes: the `paint` operator replays the strokes the editor
//! recorded, `scratches` makes random ones. Both go through one rasterizer.

use mscent_math::epsilon::SCRATCH_MIN_DIRECTION;
use mscent_math::rng::Rng;

use super::cost::Cost;
use super::stroke_work::{self, MAX_STROKES, charge_stroke};
use super::util::{BLACK, WHITE, canvas_at, over};
use super::{Ctx, Inputs, OpSpec, Role, Stroke, color, float, int, strokes};
use crate::error::TexError;
use crate::image::{Image, to_px};

/// Whether strokes are within the caps and the work rule.
fn validate(strokes: &[Stroke]) -> Result<(), TexError> {
    let within =
        strokes.len() <= MAX_STROKES && strokes.iter().try_fold(0.0, charge_stroke).is_some();
    if within {
        Ok(())
    } else {
        Err(TexError::BadParam("strokes".to_string()))
    }
}

/// Sides of the tile, powers of two, and where its window starts: strokes are
/// in tile coordinates, so a moved window sees them moved.
#[derive(Clone, Copy)]
struct Dims {
    w: usize,
    h: usize,
    origin: [f32; 2],
}

impl Dims {
    /// Index of a texel with wrap-around.
    fn index(self, x: i32, y: i32) -> usize {
        (y as u32 as usize & (self.h - 1)) * self.w + (x as u32 as usize & (self.w - 1))
    }
}

/// The picture being painted, and the coverage of the stroke being laid.
struct Canvas {
    dims: Dims,
    px: Vec<[f32; 4]>,
    cover: Vec<f32>,
}

impl Canvas {
    fn new(ctx: &Ctx) -> Self {
        let size = ctx.frame.size;
        let mut px = Vec::with_capacity(size.texels());
        for y in 0..size.height() as i32 {
            for x in 0..size.width() as i32 {
                px.push(canvas_at(ctx, x, y));
            }
        }
        Self {
            dims: Dims {
                w: size.width() as usize,
                h: size.height() as usize,
                origin: ctx.frame.origin,
            },
            px,
            cover: vec![0.0; size.texels()],
        }
    }

    fn into_image(self, ctx: &Ctx) -> Image {
        let mut px = self.px.into_iter();
        Image::from_xy(ctx.frame.size, |_, _| px.next().map_or([0; 4], to_px))
    }
}

/// Distance from `p` to the segment `a`-`b`.
fn segment_distance(p: [f32; 2], a: [f32; 2], b: [f32; 2]) -> f32 {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let len2 = dx * dx + dy * dy;
    let t = if len2 > 0.0 {
        (((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / len2).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let (ex, ey) = (p[0] - a[0] - dx * t, p[1] - a[1] - dy * t);
    (ex * ex + ey * ey).sqrt()
}

/// How much of a texel at distance `d` from the stroke's path the brush covers:
/// full inside the hard core, falling to nothing at the rim, and never sharper
/// than a texel, so thin strokes stay smooth.
fn brush(d: f32, radius: f32, hardness: f32) -> f32 {
    let ramp = (radius * (1.0 - hardness)).max(1.0);
    // The middle of the falloff sits at the same share of the radius whatever
    // the size, so two sizes agree on how wide a stroke is.
    let middle = radius * (1.0 + hardness) / 2.0;
    (0.5 + (middle - d) / ramp).clamp(0.0, 1.0)
}

/// The columns of a row that can be within `reach` of a segment: where the segment
/// passes through the band of the row, widened by the reach. Any nearer texel has
/// its nearest point of the segment in that band and within `reach` across.
fn row_span(seg: [[f32; 2]; 2], row: f32, reach: f32) -> (i32, i32) {
    let ([ax, ay], [bx, by]) = (seg[0], seg[1]);
    let (dx, dy) = (bx - ax, by - ay);
    let (from, to) = if dy == 0.0 {
        (ax, bx)
    } else {
        let along = |y: f32| ax + dx * ((y - ay) / dy).clamp(0.0, 1.0);
        (along(row - reach), along(row + reach))
    };
    (
        (from.min(to) - reach).floor() as i32,
        (from.max(to) + reach).ceil() as i32,
    )
}

/// Calls `f(index, distance)` for each texel within reach of one segment, where
/// positions are in texels. Only the rows and spans of its capsule are walked, not
/// its bounding box, so that a segment's cost is its capsule's area whichever way
/// it points. The walk is unwrapped and addressed wrapped, which is how a stroke
/// leaving one edge arrives at the other.
fn visit_segment(dims: Dims, seg: [[f32; 2]; 2], reach: f32, mut f: impl FnMut(usize, f32)) {
    let y0 = (seg[0][1].min(seg[1][1]) - reach).floor() as i32;
    let y1 = (seg[0][1].max(seg[1][1]) + reach).ceil() as i32;
    for y in y0..=y1 {
        let row = y as f32 + 0.5;
        let (x0, x1) = row_span(seg, row, reach);
        for x in x0..=x1 {
            let p = [x as f32 + 0.5, row];
            f(dims.index(x, y), segment_distance(p, seg[0], seg[1]));
        }
    }
}

/// How many texels the brush covers that the walk of a segment does not reach, in
/// a 128 px tile. For a test: only this function and the test know the question.
#[cfg(test)]
pub(super) fn covered_outside_walk(seg: [[f32; 2]; 2], radius: f32, hardness: f32) -> usize {
    use std::collections::HashSet;
    let dims = Dims {
        w: 128,
        h: 128,
        origin: [0.0, 0.0],
    };
    let mut walked = HashSet::new();
    visit_segment(dims, seg, radius + 1.0, |i, _| {
        walked.insert(i);
    });
    let span = 400;
    let mut missed = 0;
    for y in -span..span {
        for x in -span..span {
            let p = [x as f32 + 0.5, y as f32 + 0.5];
            let covered = brush(segment_distance(p, seg[0], seg[1]), radius, hardness) > 0.0;
            if covered && !walked.contains(&dims.index(x, y)) {
                missed += 1;
            }
        }
    }
    missed
}

/// A stroke's segments in texels; a lone point is a zero-length segment, a dab.
fn segments(stroke: &Stroke, dims: Dims) -> Vec<[[f32; 2]; 2]> {
    let at = |p: &[f32; 2]| {
        [
            (p[0] - dims.origin[0]) * dims.w as f32,
            (p[1] - dims.origin[1]) * dims.h as f32,
        ]
    };
    match stroke.points.as_slice() {
        [] => Vec::new(),
        [only] => vec![[at(only), at(only)]],
        points => points.windows(2).map(|p| [at(&p[0]), at(&p[1])]).collect(),
    }
}

/// Lays one stroke: coverage is the most any segment gives a texel, so joints
/// and overlaps do not double a soft or translucent stroke. A second walk over
/// the same spans composites and clears the coverage again, which keeps the
/// cost to the spans instead of a clear of the whole tile per stroke.
fn lay(canvas: &mut Canvas, stroke: &Stroke) {
    let dims = canvas.dims;
    let radius = stroke.radius * dims.w as f32;
    let reach = radius + 1.0;
    let segs = segments(stroke, dims);
    for seg in &segs {
        visit_segment(dims, *seg, reach, |i, d| {
            canvas.cover[i] = canvas.cover[i].max(brush(d, radius, stroke.hardness));
        });
    }
    for seg in &segs {
        visit_segment(dims, *seg, reach, |i, _| {
            if canvas.cover[i] > 0.0 {
                canvas.px[i] = over(canvas.px[i], stroke.color, canvas.cover[i]);
                canvas.cover[i] = 0.0;
            }
        });
    }
}

fn paint_all(ctx: &Ctx, strokes: &[Stroke]) -> Image {
    let mut canvas = Canvas::new(ctx);
    for stroke in strokes {
        lay(&mut canvas, stroke);
    }
    canvas.into_image(ctx)
}

fn paint(ctx: &Ctx) -> Result<Image, TexError> {
    let list = ctx.strokes("strokes");
    validate(list)?;
    Ok(paint_all(ctx, list))
}

/// Straight random lines. Their count, length and width are capped by the table,
/// and the worst of them sweep far less than a `paint` may.
fn scratches(ctx: &Ctx) -> Result<Image, TexError> {
    let mut rng = Rng::new(u64::from(ctx.int("seed") as u32) + 1);
    let (length, width) = (ctx.float("length"), ctx.float("width"));
    let color = ctx.color("color");
    let list: Vec<Stroke> = (0..ctx.int("count"))
        .map(|_| {
            let start = [rng.range_f32(0.0, 1.0), rng.range_f32(0.0, 1.0)];
            let (dx, dy) = (rng.range_f32(-1.0, 1.0), rng.range_f32(-1.0, 1.0));
            // A direction without a sine: a random vector, or along x if it is
            // too short to point anywhere.
            let norm = (dx * dx + dy * dy).sqrt();
            let dir = if norm > SCRATCH_MIN_DIRECTION {
                [dx / norm, dy / norm]
            } else {
                [1.0, 0.0]
            };
            let fade = rng.range_f32(0.4, 1.0);
            Stroke {
                radius: width / 2.0,
                hardness: 0.5,
                color: [color[0], color[1], color[2], color[3] * fade],
                points: vec![
                    start,
                    [start[0] + dir[0] * length, start[1] + dir[1] * length],
                ],
            }
        })
        .collect();
    Ok(paint_all(ctx, &list))
}

const OPTIONAL: Inputs = Inputs { min: 0, max: 1 };

pub(super) const OPS: &[OpSpec] = &[
    OpSpec {
        name: "paint",
        group: "Draw",
        inputs: OPTIONAL,
        params: &[color("background", BLACK), strokes("strokes")],
        role: Role::Kernel,
        kernel: paint,
        cost: Cost::Worked(stroke_work::paint_cost),
    },
    OpSpec {
        name: "scratches",
        group: "Draw",
        inputs: OPTIONAL,
        params: &[
            int("count", 0, 512, 32),
            float("length", 0.0, 0.5, 0.15),
            float("width", 0.0, 0.05, 0.004),
            int("seed", 0, 65535, 1),
            color("color", WHITE),
            color("background", BLACK),
        ],
        role: Role::Kernel,
        kernel: scratches,
        cost: Cost::Worked(stroke_work::scratches_cost),
    },
];
