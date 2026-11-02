//! A sculpting stroke, on a thread of its own: the pointer sends rays, and the
//! thread picks, grows the box and dabs, so a dab of any size costs the UI nothing.
//! The UI adopts the newest published volume whenever it ticks, which is an `Arc`
//! clone. What a stroke does to the volume is in `sculptor.rs`.

use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex, PoisonError};

use mscent_math::Vec3d;
use mscent_world::voxel::{CellSize, Clip};

use super::document::{BrushDef, Document, Id};
use super::generators::{FieldKind, Kind};
use super::growth::Request;
use super::pick::Ray;
use super::sculptor::Sculptor;

/// What a dab does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VoxelOp {
    Add,
    Carve,
    Smooth,
    Paint,
    /// Toward the plane set by the stroke's first hit.
    Flatten,
    /// Displace by seeded noise.
    Roughen,
    /// Click a path: each click carves on from the last.
    Tunnel,
    /// Click to capture a ball of the volume.
    Copy,
    /// Click to paste what Copy captured.
    Stamp,
}

impl VoxelOp {
    /// In the order the panel lists them.
    pub const ALL: [VoxelOp; 9] = [
        VoxelOp::Add,
        VoxelOp::Carve,
        VoxelOp::Smooth,
        VoxelOp::Paint,
        VoxelOp::Flatten,
        VoxelOp::Roughen,
        VoxelOp::Tunnel,
        VoxelOp::Copy,
        VoxelOp::Stamp,
    ];

    pub const NAMES: [&'static str; 9] = [
        "Add", "Carve", "Smooth", "Paint", "Flatten", "Roughen", "Tunnel", "Copy", "Stamp",
    ];
}

/// The tool's settings.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VoxelTool {
    pub op: VoxelOp,
    pub radius: f64,
    pub material: u8,
    /// The cell size of a volume a stroke creates.
    pub cell: CellSize,
    /// Seeds roughening, generators and fields.
    pub seed: u32,
    /// The generator a click grows, if one is chosen; otherwise clicks dab.
    pub generator: Option<Kind>,
    /// What Fill pours, and how wide its passages are.
    pub field: FieldKind,
    pub wavelength: f64,
    /// Where the tunnel being clicked has got to, and how wide it is there.
    pub path: Option<(Vec3d, f64)>,
}

impl Default for VoxelTool {
    fn default() -> Self {
        Self {
            op: VoxelOp::Carve,
            radius: 1.0,
            material: 4,
            cell: CellSize::Quarter,
            seed: 1,
            generator: None,
            field: FieldKind::Gyroid,
            wavelength: 12.0,
            path: None,
        }
    }
}

/// How a stroke was started.
#[derive(Clone, Debug, Default)]
pub struct StrokeKeys {
    /// Add and Carve swap.
    pub swap: bool,
    /// Pick from the volume as it now is: strokes build up.
    pub build_up: bool,
    /// What Stamp pastes.
    pub clip: Option<Arc<Clip>>,
    /// A whole job to run when the stroke ends, instead of dabbing.
    pub request: Option<Request>,
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
    clip: Option<Clip>,
    reached: Option<(Vec3d, f64)>,
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

    /// What Copy captured, once.
    pub fn take_clip(&self) -> Option<Clip> {
        self.lock().clip.take()
    }

    /// Where a tunnel got to, once.
    pub fn take_reached(&self) -> Option<(Vec3d, f64)> {
        self.lock().reached.take()
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
    let mut s = Sculptor::new(doc, &brushes, new_id, tool, op, keys.build_up, keys.clip);
    s.placing = matches!(keys.request, Some(Request::Grow { .. }));
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
        publish(&mut s, latest);
        if ended {
            break;
        }
    }
    if let Some(request) = keys.request {
        s.request(request);
    }
    s.finish();
    publish(&mut s, latest);
    s.sculpted()
}

fn publish(s: &mut Sculptor, latest: &Mutex<Latest>) {
    let mut slot = latest.lock().unwrap_or_else(PoisonError::into_inner);
    slot.sculpted = s.sculpted();
    if let Some(note) = s.note.take() {
        slot.note = Some(note);
    }
    if let Some(clip) = s.out.copied.take() {
        slot.clip = Some(clip);
    }
    if let Some(reached) = s.out.reached.take() {
        slot.reached = Some(reached);
    }
}
