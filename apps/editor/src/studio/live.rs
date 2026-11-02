//! The live path: a change that only touches voxel chunks is meshed off to the
//! side and swapped in between frames, with no CSG job and no draft bake. Today's
//! preview per edit takes 15 to 706 ms; a dab must reach the picture in 33.
//!
//! One job runs at a time and it takes the newest document, so stale work is
//! replaced and never queued, and undo during a running job is only another
//! newest document.

use std::sync::Arc;

use mscent_render::VoxelChange;
use mscent_world::Bsp;
use mscent_world::voxel::{CHUNK_EDGE, Volume, VoxelWorld};

use super::{Studio, Tick};
use crate::build::{self, Job, Light, validate};
use crate::model::Document;
use crate::model::document::BrushDef;

/// A running job, and whether its change alters the light.
pub(super) struct LiveJob {
    job: Job<VoxelChange>,
    relit: bool,
}

/// What must be meshed to go from one document to another.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum LivePlan {
    /// The same boxes: these chunks, by volume number.
    Chunks(Vec<(usize, Vec<[u16; 3]>)>),
    /// Add volumes came, went or changed box: all surfaces, still no CSG.
    All,
}

fn solid_parts_match(a: &[&BrushDef], b: &[&BrushDef]) -> bool {
    a.len() == b.len()
        && a.iter()
            .zip(b)
            .all(|(x, y)| match (x.volume(), y.volume()) {
                // Only its box is in the CSG.
                (Some(p), Some(q)) => x.id == y.id && p.same_box(q),
                (None, None) => x == y,
                _ => false,
            })
}

/// Everything in the BSP: brushes and subtract volumes.
fn in_csg(doc: &Document) -> Vec<&BrushDef> {
    let add_volume = |b: &&BrushDef| b.volume().is_some_and(|v| v.op() == mscent_world::Op::Add);
    doc.brushes().iter().filter(|b| !add_volume(b)).collect()
}

/// A chunk on the border of its volume is read by any volume touching it.
fn on_border(v: &Volume, c: [u16; 3]) -> bool {
    let n = v.chunks();
    (0..3).any(|a| c[a] == 0 || c[a] + 1 == n[a])
}

/// How to bring `old`'s picture to `new`, or `None` if the BSP must change.
pub(super) fn plan(old: &Document, new: &Document) -> Option<LivePlan> {
    let same_rest = old.entities() == new.entities()
        && old.groups() == new.groups()
        && solid_parts_match(&in_csg(old), &in_csg(new));
    if !same_rest {
        return None;
    }
    let same_list = old.brushes().len() == new.brushes().len()
        && (old.brushes().iter().zip(new.brushes())).all(|(a, b)| {
            a.id == b.id
                && match (a.volume(), b.volume()) {
                    (Some(p), Some(q)) => p.same_box(q),
                    _ => true,
                }
        });
    if !same_list {
        return Some(LivePlan::All);
    }
    let volumes: Vec<(&Volume, &Volume)> = old
        .volumes()
        .zip(new.volumes())
        .map(|((_, a), (_, b))| (a, b))
        .collect();
    let mut wanted: Vec<(usize, Vec<[u16; 3]>)> = Vec::new();
    for (i, (a, b)) in volumes.iter().enumerate() {
        let chunks = b.changed_chunks(a);
        if chunks.is_empty() {
            continue;
        }
        if chunks.iter().any(|&c| on_border(b, c)) {
            for (j, (_, other)) in volumes.iter().enumerate() {
                if j != i && other.touches(b) {
                    wanted.push((j, other.all_chunks()));
                }
            }
        }
        wanted.push((i, chunks));
    }
    Some(LivePlan::Chunks(wanted))
}

fn run_plan(doc: &Document, plan: &LivePlan, bsp: &Bsp) -> Result<VoxelChange, String> {
    let level = validate(doc)?;
    let brushes = level.static_brushes();
    Ok(match plan {
        LivePlan::All => VoxelChange::All(Box::new(VoxelWorld::build(&level, &brushes, bsp))),
        LivePlan::Chunks(wanted) => {
            VoxelChange::Chunks(VoxelWorld::remesh(&level, &brushes, bsp, wanted))
        }
    })
}

impl Studio {
    /// Tells the studio a stroke is running: it starts no bake or preview of its
    /// own until the stroke is over.
    pub fn set_stroking(&mut self, on: bool) {
        if on && !self.stroking {
            self.bake_job = None;
        }
        self.stroking = on;
    }

    /// Jobs that went through the live path.
    #[cfg(test)]
    pub fn live_jobs(&self) -> usize {
        self.live_count
    }

    /// Takes a finished live job's batch to the render thread.
    pub(super) fn poll_live(&mut self, out: &mut Tick) {
        let Some(live) = &self.live_job else {
            return;
        };
        let Some(result) = live.job.poll() else {
            return;
        };
        let (doc, relit) = (live.job.doc.clone(), live.relit);
        self.live_job = None;
        match result {
            Ok(change) => {
                if let Some(worker) = &mut self.worker {
                    worker.voxels(change);
                }
                if relit {
                    self.light = Light::Draft;
                    self.failed_bake = None;
                }
                self.error = None;
                self.built_doc = Some(doc);
                self.live_dirty = true;
            }
            Err(e) => {
                // The normal path says the same thing with more care.
                out.log.push(format!("ERROR: {e}"));
                self.wanted = None;
            }
        }
    }

    /// Starts the live path for `doc` if it can take the change. True when the
    /// change is its business, in which case the normal preview keeps out of it.
    pub(super) fn try_live(&mut self, doc: &Document) -> bool {
        let Some(pictured) = self.wanted.as_ref().filter(|_| self.worker.is_some()) else {
            return false;
        };
        if self.preview_job.is_some() || self.refresh {
            return false;
        }
        let Some(bsp) = self.live_bsp.clone() else {
            return false;
        };
        let Some(plan) = plan(pictured, doc) else {
            return false;
        };
        if self.live_job.is_none() {
            self.start_live(doc, plan, bsp);
        }
        true
    }

    fn start_live(&mut self, doc: &Document, plan: LivePlan, bsp: Arc<Bsp>) {
        let relit = !self.wanted.as_ref().is_some_and(|w| w.same_geometry(doc));
        self.wanted = Some(doc.clone());
        let job = build::spawn("mscent-live", doc, move |d| run_plan(d, &plan, &bsp));
        match job {
            Ok(job) => {
                self.live_count += 1;
                self.live_job = Some(LiveJob { job, relit });
            }
            Err(_) => self.wanted = None,
        }
    }
}

// A chunk is 16 samples on a side: the plan's padding rule depends on it.
const _: () = assert!(CHUNK_EDGE == 16);
