//! Drawing on a `paint` block: a stroke is begun, grown a point at a time while the
//! pointer is down, and ended. The whole stroke is one undo step, taken when it
//! begins, and the page shows it growing because it is in the recipe from the
//! first point.

use mscent_texgen::ops::{self, MAX_POINTS, MAX_STROKES, ParamKind, STROKE_COORD, Stroke};
use mscent_texgen::recipe::MAX_LINE;

use super::{Live, TexDoc};

/// Points are kept on a grid of this many to the tile (a step under a texel of the
/// largest tile), so a file stays small and a coordinate is a few digits.
const GRID: f32 = 4096.0;
/// The most characters a point takes in a recipe: two coordinates of the grid
/// within `STROKE_COORD`, the longest `-0.00048828125`, a comma and a space.
const POINT_CHARS: usize = 2 * 14 + 2;
/// Room for a stroke line's own numbers before its points.
const LINE_HEAD: usize = 128;
/// The most points a stroke holds. A stroke is one line of the recipe and the
/// reader refuses a line past `MAX_LINE`, so this many always fit one, and a file
/// the editor wrote always loads. Under the operator's `MAX_POINTS`, which no
/// rounding of coordinates could fit in a line.
pub const STROKE_POINTS: usize = {
    let fits = (MAX_LINE - LINE_HEAD) / POINT_CHARS;
    if fits < MAX_POINTS { fits } else { MAX_POINTS }
};

/// A coordinate on the grid.
fn snapped(c: f32) -> f32 {
    (c * GRID).round() / GRID
}

/// A stroke with every point on the grid.
fn snapped_stroke(mut stroke: Stroke) -> Stroke {
    for point in &mut stroke.points {
        *point = point.map(snapped);
    }
    stroke
}

/// A new point must be this far from the last one, as a share of the brush
/// radius, so a long stroke stays small however finely the pointer reports.
const SPACING: f32 = 0.25;
/// And at least this far, in tiles: a hairline brush would otherwise take a point
/// per pixel moved. Under a texel of the largest tile.
const MIN_SPACING: f32 = 1.0 / 1024.0;

/// What growing a stroke did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Growth {
    Added,
    /// Too near the last point, out of range, or no stroke is being drawn.
    Skipped,
    /// The stroke holds as many points as one may, and stopped growing.
    Full,
}

/// The distance two points of a stroke of this radius must be apart, in tiles.
fn spacing(radius: f32) -> f32 {
    (radius * SPACING).max(MIN_SPACING)
}

impl TexDoc {
    /// The index of a block's strokes parameter, if it has one.
    fn strokes_param(&self, block: usize) -> Result<usize, String> {
        let at = self
            .recipe
            .blocks
            .get(block)
            .ok_or_else(|| format!("there is no block {block}"))?;
        at.spec
            .params
            .iter()
            .position(|p| matches!(p.kind, ParamKind::Strokes))
            .ok_or_else(|| format!("{} takes no strokes", at.spec.name))
    }

    /// Whether a block has strokes to draw on.
    pub fn can_stroke(&self, block: usize) -> bool {
        self.strokes_param(block).is_ok()
    }

    /// How many strokes a block holds.
    pub fn stroke_count(&self, block: usize) -> usize {
        let strokes = self.strokes_param(block).ok().and_then(|i| {
            match self.recipe.blocks.get(block)?.params.values().get(i)? {
                ops::Value::Strokes(s) => Some(s.len()),
                _ => None,
            }
        });
        strokes.unwrap_or(0)
    }

    /// Starts a stroke on a block with its first point already in `stroke`, and
    /// takes the undo step. A block with no strokes to take, a stroke the operator
    /// would refuse, or one past the most strokes a block holds is an error and
    /// changes nothing.
    pub fn begin_stroke(&mut self, block: usize, stroke: Stroke) -> Result<(), String> {
        let index = self.strokes_param(block)?;
        let stroke = snapped_stroke(stroke);
        if !ops::stroke_fits(&stroke) || stroke.points.len() > STROKE_POINTS {
            return Err("that brush cannot be drawn with".to_string());
        }
        if self.stroke_count(block) >= MAX_STROKES {
            return Err(format!(
                "this block holds {MAX_STROKES} strokes, the most there can be: clear some"
            ));
        }
        self.checkpoint();
        let list = self
            .recipe
            .blocks
            .get_mut(block)
            .and_then(|b| b.params.strokes_mut(index))
            .ok_or_else(|| "the strokes went missing".to_string())?;
        list.push(stroke);
        self.live = Some(Live::Stroke(block, index));
        self.changed();
        Ok(())
    }

    /// Adds a point to the stroke being drawn. `last` takes a point that is too
    /// near the end, so a stroke ends where the pointer was let go.
    pub fn extend_stroke(&mut self, point: [f32; 2], last: bool) -> Growth {
        let Some(Live::Stroke(block, index)) = self.live else {
            return Growth::Skipped;
        };
        let Some(stroke) = self
            .recipe
            .blocks
            .get_mut(block)
            .and_then(|b| b.params.strokes_mut(index))
            .and_then(|list| list.last_mut())
        else {
            return Growth::Skipped;
        };
        let point = point.map(snapped);
        if !point.iter().all(|c| STROKE_COORD.contains(c)) {
            return Growth::Skipped;
        }
        if let Some(end) = stroke.points.last() {
            let apart = (point[0] - end[0]).hypot(point[1] - end[1]);
            let needed = if last { 0.0 } else { spacing(stroke.radius) };
            if apart <= needed {
                return Growth::Skipped;
            }
        }
        if stroke.points.len() >= STROKE_POINTS {
            return Growth::Full;
        }
        stroke.points.push(point);
        self.changed();
        Growth::Added
    }

    /// Ends the stroke being drawn. The parameter panel is made again for the
    /// new count, which it could not be while the stroke grew.
    pub fn end_stroke(&mut self) {
        if matches!(self.live, Some(Live::Stroke(..))) {
            self.live = None;
            self.panel_epoch = self.panel_epoch.wrapping_add(1);
        }
    }
}

#[cfg(test)]
#[path = "texdoc_strokes_tests.rs"]
mod tests;
