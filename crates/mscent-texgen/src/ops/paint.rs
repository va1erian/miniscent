//! Round-brush strokes: the `paint` operator replays the strokes the editor
//! recorded, `scratches` makes random ones. Both go through one rasterizer.

use std::ops::RangeInclusive;

use mscent_math::rng::Rng;

use super::util::{BLACK, WHITE, canvas_at, over};
use super::{Ctx, Inputs, OpSpec, Role, Stroke, color, float, int, strokes};
use crate::error::TexError;
use crate::image::{Image, to_px};

/// Caps on a recipe's strokes, which are untrusted.
pub const MAX_STROKES: usize = 4096;
pub const MAX_POINTS: usize = 4096;
/// Points may leave the tile so a stroke can cross an edge, but not by much: a
/// stroke started on a tile and dragged across the two tiles of the editor's
/// tiled view stays inside this.
pub const STROKE_COORD: RangeInclusive<f32> = -2.0..=3.0;
/// Brush radius, as a fraction of the tile's width.
pub const STROKE_RADIUS: RangeInclusive<f32> = 0.0..=0.5;
/// Texel visits a `paint` may make, per texel of the tile, plus a floor for
/// tiny tiles. Bounds the time a hostile recipe can take: 4096 strokes of 4096
/// long segments would otherwise cost a tile each.
const WORK_PER_TEXEL: u64 = 64;
const WORK_FLOOR: u64 = 1 << 20;

/// Whether one stroke is something `paint` will run: finite numbers in range and
/// no more points than the cap. The recipe reader and the editor ask this too, so
/// what they accept is exactly what the kernel accepts.
pub fn stroke_fits(s: &Stroke) -> bool {
    STROKE_RADIUS.contains(&s.radius)
        && (0.0..=1.0).contains(&s.hardness)
        && s.color.iter().all(|c| (0.0..=1.0).contains(c))
        && s.points.len() <= MAX_POINTS
        && s.points.iter().flatten().all(|c| STROKE_COORD.contains(c))
}

/// Whether strokes are within the caps and made of finite, sensible numbers.
fn validate(strokes: &[Stroke]) -> Result<(), TexError> {
    if strokes.len() > MAX_STROKES || !strokes.iter().all(stroke_fits) {
        return Err(TexError::BadParam("strokes".to_string()));
    }
    Ok(())
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

/// Counts texel visits against a limit.
struct Budget(u64);

impl Budget {
    fn charge(&mut self, texels: u64) -> Result<(), TexError> {
        self.0 = self
            .0
            .checked_sub(texels)
            .ok_or_else(|| TexError::BadParam("strokes".to_string()))?;
        Ok(())
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

/// Calls `f(index, distance)` for each texel of the box around one segment,
/// where positions are in texels. The box is walked unwrapped and addressed
/// wrapped, which is how a stroke leaving one edge arrives at the other.
fn visit_segment(
    dims: Dims,
    seg: [[f32; 2]; 2],
    reach: f32,
    budget: &mut Budget,
    mut f: impl FnMut(usize, f32),
) -> Result<(), TexError> {
    let low = |i: usize| seg[0][i].min(seg[1][i]) - reach;
    let high = |i: usize| seg[0][i].max(seg[1][i]) + reach;
    let (x0, x1) = (low(0).floor() as i32, high(0).ceil() as i32);
    let (y0, y1) = (low(1).floor() as i32, high(1).ceil() as i32);
    let area = (i64::from(x1 - x0) + 1) * (i64::from(y1 - y0) + 1);
    budget.charge(area as u64)?;
    for y in y0..=y1 {
        for x in x0..=x1 {
            let p = [x as f32 + 0.5, y as f32 + 0.5];
            f(dims.index(x, y), segment_distance(p, seg[0], seg[1]));
        }
    }
    Ok(())
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
/// the same boxes composites and clears the coverage again, which keeps the
/// cost to the boxes instead of a clear of the whole tile per stroke.
fn lay(canvas: &mut Canvas, stroke: &Stroke, budget: &mut Budget) -> Result<(), TexError> {
    let dims = canvas.dims;
    let radius = stroke.radius * dims.w as f32;
    let reach = radius + 1.0;
    let segs = segments(stroke, dims);
    for seg in &segs {
        visit_segment(dims, *seg, reach, budget, |i, d| {
            canvas.cover[i] = canvas.cover[i].max(brush(d, radius, stroke.hardness));
        })?;
    }
    for seg in &segs {
        visit_segment(dims, *seg, reach, budget, |i, _| {
            if canvas.cover[i] > 0.0 {
                canvas.px[i] = over(canvas.px[i], stroke.color, canvas.cover[i]);
                canvas.cover[i] = 0.0;
            }
        })?;
    }
    Ok(())
}

fn paint_all(ctx: &Ctx, strokes: &[Stroke], limit: u64) -> Result<Image, TexError> {
    let mut canvas = Canvas::new(ctx);
    let mut budget = Budget(limit);
    for stroke in strokes {
        lay(&mut canvas, stroke, &mut budget)?;
    }
    Ok(canvas.into_image(ctx))
}

fn paint(ctx: &Ctx) -> Result<Image, TexError> {
    let list = ctx.strokes("strokes");
    validate(list)?;
    let limit = WORK_FLOOR.max(ctx.frame.size.texels() as u64 * WORK_PER_TEXEL);
    paint_all(ctx, list, limit)
}

/// Straight random lines. Their count and length are capped by the table, so the
/// cost is too, and they need no budget.
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
            let dir = if norm > 0.05 {
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
    paint_all(ctx, &list, u64::MAX)
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
    },
];
