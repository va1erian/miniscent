//! A sculpting stroke, on a thread of its own: the pointer sends rays, and the
//! thread picks, grows the box and dabs, so a dab of any size costs the UI nothing.
//! The UI adopts the newest published volume whenever it ticks, which is an `Arc`
//! clone.
//!
//! Picking reads a snapshot from the start of the stroke. Otherwise Add moves the
//! surface toward the camera and the stroke climbs up its own result. A dab takes
//! the larger of field and shape, so scrubbing an area keeps one even depth; Alt
//! picks from the volume as it now is, which is how a stroke builds up instead.

use std::collections::BTreeSet;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex, PoisonError};

use mscent_math::Vec3d;
use mscent_world::voxel::{
    CHUNK_EDGE, CellSize, Chunk, Context, DabOp, MAX_DETAIL_CHUNKS, Prim, Volume, apply_dab,
    first_solid, remove_islands,
};
use mscent_world::{Brush, Op};

use super::document::{BrushDef, Document, Id, Shape};
use super::pick::{Ray, pick_face};

/// Farthest a stroke reaches along a ray.
const MAX_REACH: f64 = 512.0;
/// Most dabs between two pointer samples, so a wild move stays bounded.
const MAX_SPACED_DABS: usize = 64;

/// What a dab does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VoxelOp {
    Add,
    Carve,
    Smooth,
    Paint,
}

/// The tool's settings.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VoxelTool {
    pub op: VoxelOp,
    pub radius: f64,
    pub material: u8,
    /// The cell size of a volume a stroke creates.
    pub cell: CellSize,
}

impl Default for VoxelTool {
    fn default() -> Self {
        Self {
            op: VoxelOp::Carve,
            radius: 1.0,
            material: 4,
            cell: CellSize::Quarter,
        }
    }
}

/// How a stroke was started.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct StrokeKeys {
    /// Add and Carve swap.
    pub swap: bool,
    /// Pick from the volume as it now is: strokes build up.
    pub build_up: bool,
}

/// What the UI needs from the thread.
#[derive(Clone, Debug, PartialEq)]
pub struct Sculpted {
    pub def: BrushDef,
    /// True when `def` is not yet in the document.
    pub is_new: bool,
}

/// What the thread has for the UI since the last look.
#[derive(Debug, Default)]
struct Latest {
    sculpted: Option<Sculpted>,
    note: Option<String>,
}

enum Msg {
    Ray(Ray),
    End,
}

/// The UI's end of a running stroke.
pub struct StrokeJob {
    tx: Sender<Msg>,
    latest: Arc<Mutex<Latest>>,
    done: Receiver<Option<Sculpted>>,
}

impl std::fmt::Debug for StrokeJob {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("StrokeJob")
    }
}

impl StrokeJob {
    /// Starts a stroke on `doc` as it is now. `new_id` names a volume it creates.
    pub fn start(
        doc: Document,
        new_id: Id,
        tool: VoxelTool,
        keys: StrokeKeys,
    ) -> Result<Self, String> {
        let (tx, rx) = channel();
        let (done_tx, done) = channel();
        let latest = Arc::new(Mutex::new(Latest::default()));
        let shared = Arc::clone(&latest);
        std::thread::Builder::new()
            .name("mscent-stroke".into())
            .spawn(move || {
                let last = run(&doc, new_id, tool, keys, &rx, &shared);
                let _ = done_tx.send(last);
            })
            .map_err(|e| format!("cannot start the stroke: {e}"))?;
        Ok(Self { tx, latest, done })
    }

    pub fn ray(&self, ray: Ray) {
        let _ = self.tx.send(Msg::Ray(ray));
    }

    pub fn end(&self) {
        let _ = self.tx.send(Msg::End);
    }

    /// The newest result since the last call.
    pub fn take(&self) -> Option<Sculpted> {
        self.lock().sculpted.take()
    }

    /// What the user should be told, if anything.
    pub fn take_note(&self) -> Option<String> {
        self.lock().note.take()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Latest> {
        self.latest.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The stroke's final result once it has ended; `Some(None)` for a stroke
    /// that changed nothing.
    pub fn finished(&self) -> Option<Option<Sculpted>> {
        self.done.try_recv().ok()
    }

    /// Waits for the final result.
    #[cfg(test)]
    pub fn wait(&self) -> Option<Sculpted> {
        self.done.recv().ok().flatten()
    }
}

/// The volume under the stroke, once its first ray has found one.
struct Target {
    def: BrushDef,
    volume: Volume,
    before: Option<Volume>,
    dirty: BTreeSet<[u16; 3]>,
    last: Vec3d,
}

struct Sculptor<'a> {
    doc: &'a Document,
    brushes: &'a [Brush],
    others: Vec<(Id, Volume)>,
    picks: Vec<(Id, Context<'a>, Volume)>,
    new_id: Id,
    tool: VoxelTool,
    op: VoxelOp,
    build_up: bool,
    target: Option<Target>,
    ctx: Option<Context<'a>>,
    note: Option<String>,
}

fn run(
    doc: &Document,
    new_id: Id,
    tool: VoxelTool,
    keys: StrokeKeys,
    rx: &Receiver<Msg>,
    latest: &Mutex<Latest>,
) -> Option<Sculpted> {
    let level = doc.to_level().ok()?;
    let brushes = level.static_brushes();
    let op = match (tool.op, keys.swap) {
        (VoxelOp::Add, true) => VoxelOp::Carve,
        (VoxelOp::Carve, true) => VoxelOp::Add,
        (other, _) => other,
    };
    let mut s = Sculptor {
        doc,
        brushes: &brushes,
        others: doc.volumes().map(|(d, v)| (d.id, v.clone())).collect(),
        picks: doc
            .volumes()
            .map(|(d, v)| (d.id, Context::new(v, &brushes), v.clone()))
            .collect(),
        new_id,
        tool,
        op,
        build_up: keys.build_up,
        target: None,
        ctx: None,
        note: None,
    };
    while let Ok(msg) = rx.recv() {
        let mut ended = matches!(msg, Msg::End);
        if let Msg::Ray(ray) = msg {
            s.ray(ray);
        }
        // Whatever else has come in goes out together.
        while let Ok(more) = rx.try_recv() {
            match more {
                Msg::Ray(ray) => s.ray(ray),
                Msg::End => ended = true,
            }
        }
        s.publish(latest);
        if ended {
            break;
        }
    }
    s.finish();
    s.publish(latest);
    s.sculpted()
}

impl<'a> Sculptor<'a> {
    fn publish(&mut self, latest: &Mutex<Latest>) {
        let mut slot = latest.lock().unwrap_or_else(PoisonError::into_inner);
        slot.sculpted = self.sculpted();
        if let Some(note) = self.note.take() {
            slot.note = Some(note);
        }
    }

    fn sculpted(&self) -> Option<Sculpted> {
        let t = self.target.as_ref()?;
        let mut def = t.def.clone();
        def.op = t.volume.op();
        def.shape = Shape::Volume(t.volume.clone());
        Some(Sculpted {
            def,
            is_new: t.before.is_none(),
        })
    }

    /// Where a ray meets the world: distance, and whether it met a brush.
    fn hit(&mut self, ray: &Ray) -> Option<(f64, bool)> {
        // Building up reads the volume as it now is, and a wall the stroke has
        // dug away is no longer a surface.
        let dug = |t: f64| {
            let p = ray.origin + ray.dir * t;
            self.build_up
                && self
                    .target
                    .as_ref()
                    .is_some_and(|x| x.volume.op() == Op::Subtract && x.volume.holds(p))
        };
        let brush = pick_face(self.doc, ray).map(|h| h.t).filter(|&t| !dug(t));
        let mut voxel = None;
        let live = self.target.as_ref().filter(|_| self.build_up);
        for (id, ctx, volume) in &mut self.picks {
            if live.is_some_and(|t| t.def.id == *id) {
                continue;
            }
            if let Some(t) = first_solid(volume, ctx, ray.origin, ray.dir, MAX_REACH) {
                voxel = Some(voxel.map_or(t, |v: f64| v.min(t)));
            }
        }
        // The context follows the live volume as it grows.
        if let (Some(t), Some(ctx)) = (live, self.ctx.as_mut())
            && let Some(t) = first_solid(&t.volume, ctx, ray.origin, ray.dir, MAX_REACH)
        {
            voxel = Some(voxel.map_or(t, |v: f64| v.min(t)));
        }
        match (brush, voxel) {
            (Some(b), Some(v)) if v <= b => Some((v, false)),
            (Some(b), _) => Some((b, true)),
            (None, Some(v)) => Some((v, false)),
            (None, None) => None,
        }
    }

    fn ray(&mut self, ray: Ray) {
        let Some(unit) = ray.dir.normalized() else {
            return;
        };
        let ray = Ray {
            origin: ray.origin,
            dir: unit,
        };
        let Some((t, on_brush)) = self.hit(&ray) else {
            return;
        };
        let p = ray.origin + ray.dir * t;
        if self.target.is_none() && !self.choose_target(p, on_brush) {
            return;
        }
        self.dab_to(p);
    }

    /// The volume holding `p`, or a new one around it. Add makes an add volume;
    /// Carve on a brush wall makes a subtract volume, whose base matches the wall,
    /// so nothing changes until the first dab bites.
    fn choose_target(&mut self, p: Vec3d, on_brush: bool) -> bool {
        let found = self
            .doc
            .volumes()
            .find(|(_, v)| v.holds(p))
            .map(|(d, v)| (d.clone(), v.clone()));
        let (def, volume, before) = match found {
            Some((def, volume)) => {
                let before = volume.clone();
                let volume = self.ready(volume, on_brush);
                (def, volume, Some(before))
            }
            None => match self.new_volume(p) {
                Some((def, volume)) => (def, volume, None),
                None => return false,
            },
        };
        self.others.retain(|(id, _)| *id != def.id);
        self.ctx = Some(Context::new(&volume, self.brushes));
        self.target = Some(Target {
            def,
            volume,
            before,
            dirty: BTreeSet::new(),
            last: p,
        });
        true
    }

    /// Carving the brush under an add volume needs it to be a subtract one, with
    /// the same sculpted result.
    fn ready(&mut self, volume: Volume, on_brush: bool) -> Volume {
        if !(on_brush && self.op == VoxelOp::Carve && volume.op() == Op::Add) {
            return volume;
        }
        let target = volume.with_op(Op::Subtract);
        let mut old = Context::new(&volume, self.brushes);
        let mut new = Context::new(&target, self.brushes);
        volume.converted(Op::Subtract, &mut old, &mut new)
    }

    fn new_volume(&mut self, p: Vec3d) -> Option<(BrushDef, Volume)> {
        let op = match self.op {
            VoxelOp::Add => Op::Add,
            VoxelOp::Carve => Op::Subtract,
            VoxelOp::Smooth | VoxelOp::Paint => {
                self.note = Some("There is no volume here to work on".into());
                return None;
            }
        };
        let cell = self.tool.cell.units();
        let size = cell * CHUNK_EDGE as f64;
        let reach = self.tool.radius + 2.0 * cell;
        let at = [p.x, p.y, p.z];
        let first = at.map(|v| ((v - reach) / size).floor());
        let last = at.map(|v| ((v + reach) / size).ceil());
        let origin = first.map(|f| f as i32 * CHUNK_EDGE as i32);
        let chunks = [0, 1, 2].map(|a| ((last[a] - first[a]).max(1.0)) as u16);
        let order = self.doc.brushes().len() as u32;
        let volume = match Volume::new(op, order, self.tool.cell, origin, chunks) {
            Ok(v) => v,
            Err(_) => {
                self.note = Some("The stroke is too large or out of range".into());
                return None;
            }
        };
        if self.others.iter().any(|(_, o)| o.overlaps(&volume)) {
            self.note = Some("Another volume is in the way".into());
            return None;
        }
        let def = BrushDef {
            id: self.new_id,
            shape: Shape::Volume(volume.clone()),
            op,
            material: u16::from(self.tool.material),
            texinfos: Vec::new(),
        };
        Some((def, volume))
    }

    fn dab_to(&mut self, p: Vec3d) {
        let r = self.tool.radius;
        let Some(from) = self.target.as_ref().map(|t| t.last) else {
            return;
        };
        match self.op {
            // A capsule, as fast mouse moves must leave no gaps.
            VoxelOp::Add | VoxelOp::Carve => {
                if let Some(prim) = Prim::round_cone(from, p, r, r) {
                    self.apply(&prim);
                }
            }
            VoxelOp::Smooth | VoxelOp::Paint => {
                let d = (p - from).length();
                let steps = ((d / (r * 0.25)).ceil() as usize).clamp(1, MAX_SPACED_DABS);
                for k in 1..=steps {
                    let at = from + (p - from) * (k as f64 / steps as f64);
                    if let Some(prim) = Prim::sphere(at, r) {
                        self.apply(&prim);
                    }
                }
            }
        }
        if let Some(t) = &mut self.target {
            t.last = p;
        }
    }

    fn apply(&mut self, prim: &Prim) {
        self.grow_for(prim);
        let op = match self.op {
            VoxelOp::Add => DabOp::Add {
                material: self.tool.material,
            },
            VoxelOp::Carve => DabOp::Carve,
            VoxelOp::Smooth => DabOp::Smooth,
            VoxelOp::Paint => DabOp::Paint {
                material: self.tool.material,
            },
        };
        let (Some(t), Some(ctx)) = (self.target.as_mut(), self.ctx.as_mut()) else {
            return;
        };
        let kept = t.volume.clone();
        let chunks = apply_dab(&mut t.volume, ctx, prim, op);
        if detail_chunks(&t.volume) + other_detail(&self.others) > MAX_DETAIL_CHUNKS {
            t.volume = kept;
            self.note = Some("The level has as much sculpted detail as it can hold".into());
            return;
        }
        t.dirty.extend(chunks);
    }

    /// Dabbing past the box grows it by whole chunks, unless a neighbour stands
    /// there: then the dab stops at the border.
    fn grow_for(&mut self, prim: &Prim) {
        if matches!(self.op, VoxelOp::Smooth | VoxelOp::Paint) {
            return;
        }
        let Some(t) = self.target.as_mut() else {
            return;
        };
        let (lo, hi) = prim.bounds();
        let m = 2.0 * t.volume.cell().units();
        let margin = Vec3d::new(m, m, m);
        let grown = match t.volume.grown_to_hold(lo - margin, hi + margin) {
            Ok(Some(g)) => g,
            Ok(None) => return,
            Err(_) => {
                self.note = Some("The volume has reached its largest size".into());
                return;
            }
        };
        if self.others.iter().any(|(_, o)| o.overlaps(&grown)) {
            self.note = Some("Another volume is in the way".into());
            return;
        }
        // The chunks moved in the table, so what was dirty is no longer named.
        t.dirty = grown.all_chunks().into_iter().collect();
        self.ctx = Some(Context::new(&grown, self.brushes));
        t.volume = grown;
    }

    /// Clears the crumbs a stroke leaves.
    fn finish(&mut self) {
        let (Some(t), Some(ctx)) = (self.target.as_mut(), self.ctx.as_mut()) else {
            return;
        };
        let dirty: Vec<[u16; 3]> = t.dirty.iter().copied().collect();
        remove_islands(&mut t.volume, ctx, &dirty);
    }
}

fn detail_chunks(volume: &Volume) -> usize {
    volume
        .all_chunks()
        .into_iter()
        .filter(|&c| matches!(volume.chunk(c), Some(Chunk::Detail(_))))
        .count()
}

fn other_detail(others: &[(Id, Volume)]) -> usize {
    others.iter().map(|(_, v)| detail_chunks(v)).sum()
}
