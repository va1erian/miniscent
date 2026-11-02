//! The Voxel tool on the editor state. One stroke is one undo step, and Esc puts
//! back what the stroke started from. A generator's result is held, already in
//! the document, until Commit makes it a step; re-rolling or Esc puts it back.

use std::sync::Arc;

use super::command::Command;
use super::document::{BrushDef, Document, Id, Shape};
use super::growth::Request;
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
    /// What a held generation was asked for and where, to roll it again.
    held: Option<(Ray, Request)>,
    /// A held generation has finished and waits for Commit.
    ready: bool,
}

impl EditorState {
    /// Button down in the 3D view: the first ray of a stroke, or the place a
    /// generator will grow.
    pub fn begin_stroke(&mut self, ray: Ray, keys: StrokeKeys) -> bool {
        self.cancel_stroke();
        let held = self.voxel.generator.map(|kind| {
            let request = Request::Grow {
                kind,
                seed: self.voxel.seed,
            };
            (ray, request)
        });
        let keys = StrokeKeys {
            clip: self.clip.clone(),
            request: held.map(|(_, r)| r).or(keys.request),
            ..keys
        };
        if !self.start_stroke(keys, held) {
            return false;
        }
        if let Some(s) = &self.stroke {
            s.job.ray(ray);
        }
        true
    }

    fn start_stroke(&mut self, keys: StrokeKeys, held: Option<(Ray, Request)>) -> bool {
        let new_id = self.doc.alloc_id();
        let job = match StrokeJob::start(self.doc.clone(), new_id, self.voxel, keys) {
            Ok(job) => job,
            Err(e) => {
                self.message = e;
                return false;
            }
        };
        self.stroke = Some(Stroke {
            job,
            before: None,
            ending: false,
            new_id,
            held,
            ready: false,
        });
        true
    }

    /// Pours the chosen field into the selected volume: one undo step.
    pub fn fill_selected(&mut self) -> bool {
        let Some(Selection::Brush(id)) = self.selection.filter(|s| match s {
            Selection::Brush(id) => self.doc.volumes().any(|(d, _)| d.id == *id),
            Selection::Entity(_) => false,
        }) else {
            self.message = "Select a volume to fill".into();
            return false;
        };
        self.cancel_stroke();
        let tool = self.voxel;
        let request = Request::Fill {
            id,
            field: tool.field,
            seed: tool.seed,
            wavelength: tool.wavelength,
        };
        let keys = StrokeKeys {
            request: Some(request),
            ..Default::default()
        };
        if !self.start_stroke(keys, None) {
            return false;
        }
        self.end_stroke();
        true
    }

    /// The pointer moved during a stroke.
    pub fn stroke_ray(&mut self, ray: Ray) {
        if let Some(s) = &mut self.stroke {
            s.job.ray(ray);
            if let Some((last, _)) = &mut s.held {
                *last = ray;
            }
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

    /// Whether the pointer is still working a stroke. A held generation is not.
    pub fn stroking(&self) -> bool {
        self.stroke.as_ref().is_some_and(|s| !s.ready)
    }

    /// Whether a generated result waits for Commit.
    pub fn pending(&self) -> bool {
        self.stroke.as_ref().is_some_and(|s| s.ready)
    }

    /// Takes in what the stroke has done so far, and records it when it is over.
    /// Costs an `Arc` clone, which is why this may run on every UI tick.
    pub fn poll_stroke(&mut self) -> bool {
        let Some(stroke) = &self.stroke else {
            return false;
        };
        if stroke.ready {
            return false;
        }
        // Asked first: whatever the thread published before it finished is then
        // surely here to take.
        let finished = stroke.ending.then(|| stroke.job.finished()).flatten();
        let latest = stroke.job.take();
        let note = stroke.job.take_note();
        let clip = stroke.job.take_clip();
        let reached = stroke.job.take_reached();
        if let Some(note) = note {
            self.message = note;
        }
        if let Some(clip) = clip {
            self.message = "Copied".into();
            self.clip = Some(Arc::new(clip));
        }
        if reached.is_some() {
            self.voxel.path = reached;
        }
        let mut changed = latest.is_some_and(|s| self.adopt_sculpted(&s));
        if let Some(last) = finished {
            if let Some(sculpted) = last {
                self.adopt_sculpted(&sculpted);
            }
            self.settle_stroke();
            changed = true;
        }
        changed
    }

    /// A finished stroke is recorded; a finished generation waits for Commit.
    fn settle_stroke(&mut self) {
        let grown = self.stroke.as_mut().and_then(|s| {
            s.ready = s.held.is_some();
            match s.held {
                Some((_, Request::Grow { kind, .. })) => Some(kind.name()),
                _ => None,
            }
        });
        match grown {
            Some(name) => {
                self.message =
                    format!("{name}: Commit keeps it, the wheel rolls another, Esc drops it");
            }
            None => self.record_stroke(),
        }
    }

    /// Ends the stroke and waits for it.
    #[cfg(test)]
    pub fn finish_stroke(&mut self) {
        self.end_stroke();
        let last = self.stroke.as_ref().and_then(|s| s.job.wait());
        if let Some(sculpted) = last {
            self.adopt_sculpted(&sculpted);
        }
        if let Some(s) = &self.stroke {
            if let Some(note) = s.job.take_note() {
                self.message = note;
            }
            if let Some(clip) = s.job.take_clip() {
                self.clip = Some(Arc::new(clip));
            }
            if let Some(reached) = s.job.take_reached() {
                self.voxel.path = Some(reached);
            }
        }
        self.settle_stroke();
    }

    /// Keeps the generated result as one undo step.
    pub fn commit_pending(&mut self) -> bool {
        if !self.pending() {
            return false;
        }
        self.record_stroke();
        true
    }

    /// Rolls the held generation again with `seed`.
    pub fn reroll_to(&mut self, seed: u32) -> bool {
        let Some((ray, Request::Grow { kind, .. })) = self.stroke.as_ref().and_then(|s| s.held)
        else {
            return false;
        };
        self.voxel.seed = seed;
        self.voxel.generator = Some(kind);
        self.begin_stroke(ray, StrokeKeys::default());
        self.end_stroke();
        true
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

    /// Ends the tunnel being clicked, so the next click starts a new one.
    pub fn end_path(&mut self) {
        if self.voxel.path.take().is_some() && self.voxel.op == VoxelOp::Tunnel {
            self.message = "Tunnel ended".into();
        }
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
