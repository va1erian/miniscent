//! The Voxel tool on the editor state. One stroke is one undo step, and Esc puts
//! back what the stroke started from.

use super::command::Command;
use super::document::{BrushDef, Document, Id, Shape};
use super::pick::Ray;
use super::state::{EditorState, Selection};
use super::stroke::{Sculpted, StrokeJob, StrokeKeys, VoxelOp};

/// A stroke in progress.
#[derive(Debug)]
pub struct Stroke {
    job: StrokeJob,
    /// The volume as it was, or `None` for one the stroke created.
    before: Option<BrushDef>,
    /// The stroke has its last ray and is finishing.
    ending: bool,
    new_id: Id,
}

impl EditorState {
    /// Button down in the 3D view: the first ray of a stroke.
    pub fn begin_stroke(&mut self, ray: Ray, keys: StrokeKeys) -> bool {
        self.cancel_stroke();
        let new_id = self.doc.alloc_id();
        let job = match StrokeJob::start(self.doc.clone(), new_id, self.voxel, keys) {
            Ok(job) => job,
            Err(e) => {
                self.message = e;
                return false;
            }
        };
        job.ray(ray);
        self.stroke = Some(Stroke {
            job,
            before: None,
            ending: false,
            new_id,
        });
        true
    }

    /// The pointer moved during a stroke.
    pub fn stroke_ray(&mut self, ray: Ray) {
        if let Some(s) = &self.stroke {
            s.job.ray(ray);
        }
    }

    /// Button up. The stroke finishes on its own thread; [`Self::poll_stroke`]
    /// records it when it is done.
    pub fn end_stroke(&mut self) {
        if let Some(s) = &mut self.stroke {
            s.ending = true;
            s.job.end();
        }
    }

    pub fn stroking(&self) -> bool {
        self.stroke.is_some()
    }

    /// Takes in what the stroke has done so far, and records it when it is over.
    /// Costs an `Arc` clone, which is why this may run on every UI tick.
    pub fn poll_stroke(&mut self) -> bool {
        let Some(stroke) = &self.stroke else {
            return false;
        };
        let latest = stroke.job.take();
        let note = stroke.job.take_note();
        let finished = stroke.ending.then(|| stroke.job.finished()).flatten();
        if let Some(note) = note {
            self.message = note;
        }
        let mut changed = latest.is_some_and(|s| self.adopt_sculpted(&s));
        if let Some(last) = finished {
            if let Some(sculpted) = last {
                self.adopt_sculpted(&sculpted);
            }
            self.record_stroke();
            changed = true;
        }
        changed
    }

    /// Ends the stroke and waits for it (tests and scripted runs).
    pub fn finish_stroke(&mut self) {
        self.end_stroke();
        let last = self.stroke.as_ref().and_then(|s| s.job.wait());
        if let Some(sculpted) = last {
            self.adopt_sculpted(&sculpted);
        }
        if let Some(note) = self.stroke.as_ref().and_then(|s| s.job.take_note()) {
            self.message = note;
        }
        self.record_stroke();
    }

    /// Esc: the document goes back to the stroke's start.
    pub fn cancel_stroke(&mut self) -> bool {
        let Some(s) = self.stroke.take() else {
            return false;
        };
        s.job.end();
        match s.before {
            Some(before) => self.doc.replace_brush(&before),
            None => {
                self.doc.remove_brush(s.new_id);
            }
        }
        self.drop_stale_selection_after_stroke();
        self.message = "Stroke cancelled".into();
        true
    }

    fn drop_stale_selection_after_stroke(&mut self) {
        if let Some(Selection::Brush(id)) = self.selection
            && self.doc.brush(id).is_none()
        {
            self.selection = None;
        }
    }

    fn adopt_sculpted(&mut self, sculpted: &Sculpted) -> bool {
        if sculpted.is_new {
            if self.doc.brush(sculpted.def.id).is_none() {
                let index = self.doc.brushes().len();
                self.doc.insert_brush(index, sculpted.def.clone());
                self.selection = Some(Selection::Brush(sculpted.def.id));
                return true;
            }
        } else if let Some(s) = &mut self.stroke
            && s.before.is_none()
        {
            s.before = self.doc.brush(sculpted.def.id).cloned();
        }
        let same = self.doc.brush(sculpted.def.id) == Some(&sculpted.def);
        if !same {
            self.doc.replace_brush(&sculpted.def);
        }
        !same
    }

    fn record_stroke(&mut self) {
        let Some(s) = self.stroke.take() else {
            return;
        };
        match s.before {
            Some(before) => {
                if let Some(after) = self.doc.brush(before.id).cloned()
                    && after != before
                {
                    self.history.record(Command::EditBrush { before, after });
                    self.message = "Sculpted".into();
                }
            }
            None => {
                if let Some(index) = self.doc.brush_index(s.new_id)
                    && let Some(brush) = self.doc.brush(s.new_id).cloned()
                {
                    self.history.record(Command::AddBrush { index, brush });
                    self.message = "Sculpted a new volume".into();
                }
            }
        }
    }

    /// The name of the dab the tool makes, for the status bar.
    pub fn voxel_op_name(&self) -> &'static str {
        match self.voxel.op {
            VoxelOp::Add => "Add",
            VoxelOp::Carve => "Carve",
            VoxelOp::Smooth => "Smooth",
            VoxelOp::Paint => "Paint",
        }
    }
}

impl BrushDef {
    /// The volume this entry is, if it is one.
    pub fn volume(&self) -> Option<&mscent_world::voxel::Volume> {
        match &self.shape {
            Shape::Volume(v) => Some(v),
            _ => None,
        }
    }

    pub fn is_volume(&self) -> bool {
        self.volume().is_some()
    }
}

impl Document {
    /// The volumes in list order.
    pub fn volumes(&self) -> impl Iterator<Item = (&BrushDef, &mscent_world::voxel::Volume)> {
        self.brushes()
            .iter()
            .filter_map(|b| b.volume().map(|v| (b, v)))
    }
}
