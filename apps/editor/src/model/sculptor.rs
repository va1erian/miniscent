//! What a stroke does to a volume, on the stroke's thread: picks against a
//! snapshot, chooses or creates the volume, grows its box and dabs.
//!
//! Picking reads a snapshot from the start of the stroke. Otherwise Add moves the
//! surface toward the camera and the stroke climbs up its own result. A dab takes
//! the larger of field and shape, so scrubbing an area keeps one even depth; Alt
//! picks from the volume as it now is, which is how a stroke builds up instead.

use std::collections::BTreeSet;
use std::sync::Arc;

use mscent_math::Vec3d;
use mscent_world::voxel::{
    CHUNK_EDGE, Chunk, Clip, Context, DabOp, MAX_DETAIL_CHUNKS, Prim, Volume, apply_dab, capture,
    first_solid, remove_islands, stamp,
};
use mscent_world::{Brush, Op};

use super::document::{BrushDef, Document, Id, Shape};
use super::pick::{Ray, pick_face};
use super::stroke::{Sculpted, VoxelOp, VoxelTool};

/// Farthest a stroke reaches along a ray.
const MAX_REACH: f64 = 512.0;
/// Most dabs between two pointer samples, so a wild move stays bounded.
const MAX_SPACED_DABS: usize = 64;
/// How much one roughening dab moves the surface, in samples. Dabs overlap
/// several deep, so a scrub adds up to a cell or so.
const ROUGHEN_AMPLITUDE: u8 = 6;

/// The volume under the stroke, once its first ray has found one.
pub(super) struct Target {
    pub def: BrushDef,
    pub volume: Volume,
    pub before: Option<Volume>,
    pub dirty: BTreeSet<[u16; 3]>,
    pub last: Vec3d,
}

/// What the stroke leaves for the tool to remember.
#[derive(Default)]
pub(super) struct Outcome {
    /// Where a tunnel got to and how wide it is there.
    pub reached: Option<(Vec3d, f64)>,
    /// What Copy captured.
    pub copied: Option<Clip>,
}

pub(super) struct Sculptor<'a> {
    pub doc: &'a Document,
    pub brushes: &'a [Brush],
    pub others: Vec<(Id, Volume)>,
    picks: Vec<(Id, Context<'a>, Volume)>,
    pub new_id: Id,
    pub tool: VoxelTool,
    pub op: VoxelOp,
    build_up: bool,
    pub target: Option<Target>,
    pub ctx: Option<Context<'a>>,
    pub note: Option<String>,
    pub clip: Option<Arc<Clip>>,
    pub out: Outcome,
    /// Where the latest ray met the world, for generators.
    pub site: Option<(Vec3d, Vec3d)>,
    /// Whether the rays only choose a place, for a generator to build at.
    pub placing: bool,
    /// The plane Flatten works toward: a point and the way the surface faces.
    plane: Option<(Vec3d, Vec3d)>,
    /// Tunnel, Copy and Stamp act once per stroke.
    acted: bool,
}

impl<'a> Sculptor<'a> {
    pub fn new(
        doc: &'a Document,
        brushes: &'a [Brush],
        new_id: Id,
        tool: VoxelTool,
        op: VoxelOp,
        build_up: bool,
        clip: Option<Arc<Clip>>,
    ) -> Self {
        Self {
            doc,
            brushes,
            others: doc.volumes().map(|(d, v)| (d.id, v.clone())).collect(),
            picks: doc
                .volumes()
                .map(|(d, v)| (d.id, Context::new(v, brushes), v.clone()))
                .collect(),
            new_id,
            tool,
            op,
            build_up,
            target: None,
            ctx: None,
            note: None,
            clip,
            out: Outcome::default(),
            site: None,
            placing: false,
            plane: None,
            acted: false,
        }
    }

    pub fn sculpted(&self) -> Option<Sculpted> {
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

    /// Which way the surface faces where `ray` meets it.
    fn facing(&mut self, ray: &Ray) -> Vec3d {
        let spread = (self.tool.radius * 0.3).max(self.tool.cell.units());
        super::generators::surface_normal(ray, spread, |r| self.hit(r).map(|(t, _)| t))
    }

    pub fn ray(&mut self, ray: Ray) {
        let Some(unit) = ray.dir.normalized() else {
            return;
        };
        let ray = Ray {
            origin: ray.origin,
            dir: unit,
        };
        let Some((t, on_brush)) = self.hit(&ray) else {
            // Mid-stroke the pointer may leave the surface; at the start it says why.
            if self.target.is_none() {
                self.note = Some(format!("Nothing under the pointer within {MAX_REACH} units"));
            }
            return;
        };
        let p = ray.origin + ray.dir * t;
        if self.placing {
            let normal = self.facing(&ray);
            self.site = Some((p, normal));
            return;
        }
        if self.op == VoxelOp::Stamp && self.clip.is_none() {
            self.note = Some("Copy something first".into());
            return;
        }
        if self.target.is_none() {
            let reach = Vec3d::new(1.0, 1.0, 1.0) * self.tool.radius;
            if !self.choose_target(p, on_brush, (p - reach, p + reach)) {
                return;
            }
            if self.op == VoxelOp::Flatten {
                self.plane = Some((p, self.facing(&ray)));
            }
        }
        self.dab_to(p);
    }

    /// The volume holding `p`, or a new one around the box `lo..hi`. Add makes an
    /// add volume; Carve on a brush wall makes a subtract volume, whose base
    /// matches the wall, so nothing changes until the first dab bites.
    pub fn choose_target(&mut self, p: Vec3d, on_brush: bool, (lo, hi): (Vec3d, Vec3d)) -> bool {
        let found = self
            .doc
            .volumes()
            .find(|(_, v)| v.holds(p))
            .map(|(d, v)| (d.clone(), v.clone()));
        match found {
            Some((def, volume)) => {
                let before = volume.clone();
                let volume = self.ready(volume, on_brush);
                self.adopt(def, volume, Some(before), p)
            }
            None => match self.new_volume(lo, hi) {
                Some((def, volume)) => self.adopt(def, volume, None, p),
                None => false,
            },
        }
    }

    /// Works on the volume `id` of the document, wherever it is.
    pub fn choose_volume(&mut self, id: Id) -> bool {
        let Some((def, volume)) = self
            .doc
            .volumes()
            .find(|(d, _)| d.id == id)
            .map(|(d, v)| (d.clone(), v.clone()))
        else {
            self.note = Some("Select a volume to fill".into());
            return false;
        };
        let (lo, hi) = volume.bounds();
        self.adopt(def, volume.clone(), Some(volume), (lo + hi) * 0.5)
    }

    pub(super) fn adopt(
        &mut self,
        def: BrushDef,
        volume: Volume,
        before: Option<Volume>,
        p: Vec3d,
    ) -> bool {
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

    /// Whether the dab digs, which is what makes a subtract volume.
    fn digs(&self) -> bool {
        matches!(self.op, VoxelOp::Carve | VoxelOp::Tunnel)
    }

    /// Carving the brush under an add volume needs it to be a subtract one, with
    /// the same sculpted result.
    fn ready(&mut self, volume: Volume, on_brush: bool) -> Volume {
        if !(on_brush && self.digs() && volume.op() == Op::Add) {
            return volume;
        }
        let target = volume.with_op(Op::Subtract);
        let mut old = Context::new(&volume, self.brushes);
        let mut new = Context::new(&target, self.brushes);
        volume.converted(Op::Subtract, &mut old, &mut new)
    }

    fn new_volume(&mut self, lo: Vec3d, hi: Vec3d) -> Option<(BrushDef, Volume)> {
        let op = match self.op {
            // A generator builds, so its volume only adds.
            _ if self.placing => Op::Add,
            VoxelOp::Add | VoxelOp::Stamp => Op::Add,
            VoxelOp::Carve | VoxelOp::Tunnel => Op::Subtract,
            _ => {
                self.note = Some("There is no volume here to work on".into());
                return None;
            }
        };
        let cell = self.tool.cell.units();
        let size = cell * CHUNK_EDGE as f64;
        let margin = 2.0 * cell;
        let first = [lo.x, lo.y, lo.z].map(|v| ((v - margin) / size).floor());
        let last = [hi.x, hi.y, hi.z].map(|v| ((v + margin) / size).ceil());
        let origin = first.map(|f| (f as i32).saturating_mul(CHUNK_EDGE as i32));
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
            VoxelOp::Smooth | VoxelOp::Paint | VoxelOp::Flatten | VoxelOp::Roughen => {
                let d = (p - from).length();
                let steps = ((d / (r * 0.25)).ceil() as usize).clamp(1, MAX_SPACED_DABS);
                for k in 1..=steps {
                    let at = from + (p - from) * (k as f64 / steps as f64);
                    if let Some(prim) = Prim::sphere(at, r) {
                        self.apply(&prim);
                    }
                }
            }
            VoxelOp::Tunnel | VoxelOp::Copy | VoxelOp::Stamp if !self.acted => {
                self.acted = true;
                self.once(p);
            }
            VoxelOp::Tunnel | VoxelOp::Copy | VoxelOp::Stamp => {}
        }
        if let Some(t) = &mut self.target {
            t.last = p;
        }
    }

    /// The tools that act at a click rather than along a drag.
    fn once(&mut self, p: Vec3d) {
        match self.op {
            VoxelOp::Tunnel => self.tunnel(p),
            VoxelOp::Copy => self.copy(p),
            _ => self.paste(p),
        }
    }

    /// Carves from the last path point to this one, so each click digs on: a
    /// click on the bottom of the last bite is a radius deeper. The first click
    /// carves the mouth.
    fn tunnel(&mut self, p: Vec3d) {
        let r = self.tool.radius;
        let prim = match self.tool.path {
            Some((from, r0)) => Prim::round_cone(from, p, r0, r),
            None => Prim::sphere(p, r),
        };
        if let Some(prim) = prim {
            self.apply(&prim);
            self.out.reached = Some((p, r));
        }
    }

    fn copy(&mut self, p: Vec3d) {
        let (Some(t), Some(ctx)) = (self.target.as_ref(), self.ctx.as_mut()) else {
            return;
        };
        self.out.copied = capture(&t.volume, ctx, p, self.tool.radius);
        if self.out.copied.is_none() {
            self.note = Some("Nothing could be copied there".into());
        }
    }

    fn paste(&mut self, p: Vec3d) {
        let Some(clip) = self.clip.clone() else {
            return;
        };
        if self
            .target
            .as_ref()
            .is_none_or(|t| t.volume.cell() != clip.cell())
        {
            self.note = Some("The copy was made at another cell size".into());
            return;
        }
        let reach = Vec3d::new(1.0, 1.0, 1.0) * clip.reach();
        self.grow_to(p - reach, p + reach);
        self.edit(|volume, ctx| stamp(volume, ctx, &clip, p));
    }

    fn dab_op(&self) -> Option<DabOp> {
        let material = self.tool.material;
        Some(match self.op {
            VoxelOp::Add => DabOp::Add { material },
            VoxelOp::Carve | VoxelOp::Tunnel => DabOp::Carve,
            VoxelOp::Smooth => DabOp::Smooth,
            VoxelOp::Paint => DabOp::Paint { material },
            VoxelOp::Flatten => {
                let (origin, normal) = self.plane?;
                DabOp::Flatten {
                    material,
                    origin,
                    normal,
                }
            }
            VoxelOp::Roughen => DabOp::Roughen {
                seed: self.tool.seed,
                amplitude: ROUGHEN_AMPLITUDE,
                material,
            },
            VoxelOp::Copy | VoxelOp::Stamp => return None,
        })
    }

    fn apply(&mut self, prim: &Prim) {
        if let Some(op) = self.dab_op() {
            self.apply_op(prim, op);
        }
    }

    /// One dab of any kind: grows the box for shapes that add or dig, then edits.
    pub fn apply_op(&mut self, prim: &Prim, op: DabOp) {
        if matches!(op, DabOp::Add { .. } | DabOp::Carve) {
            let (lo, hi) = prim.bounds();
            self.grow_to(lo, hi);
        }
        self.edit(|volume, ctx| apply_dab(volume, ctx, prim, op));
    }

    /// Runs an edit on the volume and keeps it only if the level can hold the
    /// detail it leaves.
    pub fn edit(&mut self, change: impl FnOnce(&mut Volume, &mut Context<'a>) -> Vec<[u16; 3]>) {
        let (Some(t), Some(ctx)) = (self.target.as_mut(), self.ctx.as_mut()) else {
            return;
        };
        let kept = t.volume.clone();
        let chunks = change(&mut t.volume, ctx);
        if detail_chunks(&t.volume) + other_detail(&self.others) > MAX_DETAIL_CHUNKS {
            t.volume = kept;
            self.note = Some("The level has as much sculpted detail as it can hold".into());
            return;
        }
        t.dirty.extend(chunks);
    }

    /// Reaching past the box grows it by whole chunks, unless a neighbour stands
    /// there: then the edit stops at the border.
    fn grow_to(&mut self, lo: Vec3d, hi: Vec3d) {
        let Some(t) = self.target.as_mut() else {
            return;
        };
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
        // The chunks moved in the table: what was dirty goes with them.
        let shift = grown.chunks_before(&t.volume);
        t.dirty = t
            .dirty
            .iter()
            .map(|c| [0, 1, 2].map(|a| c[a] + shift[a]))
            .collect();
        self.ctx = Some(Context::new(&grown, self.brushes));
        t.volume = grown;
    }

    /// Clears the crumbs a stroke leaves.
    pub fn finish(&mut self) {
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
