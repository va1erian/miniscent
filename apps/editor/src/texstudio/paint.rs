//! The paint tool. Its brush and the stroke in progress are the workspace's, not
//! the document's: the brush is never an undo step, and what is undone is the
//! stroke it drew. Pointer positions arrive in tiles of the picture on show
//! (`0..1` across one tile, `0..2` across the two of the tiled view), and a stroke
//! is kept in one run of coordinates that may leave `0..1`, so that one crossing
//! an edge continues on the other side when the operator wraps it.

use mscent_texgen::ops::{STROKE_COORD, STROKE_RADIUS, Stroke};

use super::{Tab, TexStudio};
use crate::model::texdoc::{Growth, STROKE_POINTS};

/// The brush new strokes are drawn with.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Brush {
    /// Fraction of the tile's width.
    pub radius: f32,
    pub hardness: f32,
    pub color: [f32; 3],
}

impl Default for Brush {
    fn default() -> Self {
        Self {
            radius: 0.03,
            hardness: 0.6,
            color: [1.0, 1.0, 1.0],
        }
    }
}

impl Brush {
    /// The brush with every number inside what a stroke may hold.
    pub fn clamped(self) -> Self {
        // Not a number reads as the low end; `clamp` would pass it through.
        let fit = |v: f32, low: f32, high: f32| if v.is_nan() { low } else { v.clamp(low, high) };
        let unit = |v: f32| fit(v, 0.0, 1.0);
        Self {
            radius: fit(self.radius, *STROKE_RADIUS.start(), *STROKE_RADIUS.end()),
            hardness: unit(self.hardness),
            color: self.color.map(unit),
        }
    }
}

/// The pointer on the picture, in tiles from its top left corner.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Stroking {
    Down([f32; 2]),
    Move([f32; 2]),
    Up([f32; 2]),
}

/// A stroke being drawn.
#[derive(Clone, Copy, Debug)]
struct Drag {
    /// Where the pointer went down, and where on the tile that is.
    down: [f32; 2],
    start: [f32; 2],
    /// Said once that the stroke is full, not at every move after.
    told: bool,
}

/// The tool's state.
#[derive(Default)]
pub(super) struct Tool {
    brush: Brush,
    drag: Option<Drag>,
}

impl Drag {
    /// The point of the tile the pointer is over: where the stroke began plus how
    /// far the pointer has gone, which never wraps and so never jumps.
    fn point(&self, at: [f32; 2]) -> [f32; 2] {
        let clamp = |v: f32| v.clamp(*STROKE_COORD.start(), *STROKE_COORD.end());
        [
            clamp(self.start[0] + at[0] - self.down[0]),
            clamp(self.start[1] + at[1] - self.down[1]),
        ]
    }
}

impl TexStudio {
    pub fn brush(&self) -> Brush {
        self.tool.brush
    }

    /// Changes the brush for the strokes to come; nothing already drawn changes.
    pub fn set_brush(&mut self, brush: Brush) {
        self.tool.brush = brush.clamped();
    }

    /// Whether a drag on the picture draws: the Block tab shows the selected block,
    /// which has strokes.
    pub fn can_paint(&self) -> bool {
        self.tab == Tab::Block
            && self
                .doc()
                .is_some_and(|d| d.selected().is_some_and(|b| d.can_stroke(b)))
    }

    /// The pointer did something on the picture.
    pub fn paint(&mut self, event: Stroking) {
        match event {
            Stroking::Down(at) => self.begin_stroke(at),
            Stroking::Move(at) => self.grow_stroke(at, false),
            Stroking::Up(at) => {
                self.grow_stroke(at, true);
                self.tool.drag = None;
                if let Some(doc) = self.doc_mut() {
                    doc.end_stroke();
                }
                // The drag is over: the picture goes to full size.
                self.note_drag(false);
            }
        }
    }

    fn begin_stroke(&mut self, at: [f32; 2]) {
        if !self.can_paint() {
            return;
        }
        let Some(block) = self.doc().and_then(|d| d.selected()) else {
            return;
        };
        // Where on the tile: the view may show it twice.
        let start = at.map(|v| v.rem_euclid(1.0));
        let brush = self.tool.brush;
        let stroke = Stroke {
            radius: brush.radius,
            hardness: brush.hardness,
            color: [brush.color[0], brush.color[1], brush.color[2], 1.0],
            points: vec![start],
        };
        if self.edit(|d| d.begin_stroke(block, stroke)).is_some() {
            self.tool.drag = Some(Drag {
                down: at,
                start,
                told: false,
            });
            self.note_drag(true);
        }
    }

    fn grow_stroke(&mut self, at: [f32; 2], last: bool) {
        let Some(mut drag) = self.tool.drag else {
            return;
        };
        let point = drag.point(at);
        let growth = self
            .doc_mut()
            .map_or(Growth::Skipped, |d| d.extend_stroke(point, last));
        if growth == Growth::Full && !drag.told {
            drag.told = true;
            self.tool.drag = Some(drag);
            self.say(format!(
                "this stroke holds {} points, the most there can be: it stops growing here",
                STROKE_POINTS
            ));
        }
        if !last {
            self.note_drag(true);
        }
    }
}
