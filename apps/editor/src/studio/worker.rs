//! The render thread. It owns a [`Scene`] (the built level and its game
//! session), steps and renders it at a 60 Hz target and publishes the newest
//! frame. The UI thread never waits for it: commands go in through a bounded
//! channel with `try_send`, input through an inbox it only `try_lock`s
//! (`input.rs`), frames come out through a one-slot mailbox read with
//! `try_lock`, and a slow scene costs resolution (the adaptive scale),
//! not UI responsiveness. Stale frames are replaced in the mailbox, never
//! queued. A panic in the scene is caught and reported as an error text. A new
//! scene replaces the old one on the running thread, so the fly camera and any
//! held key carry straight over an edit.

use std::collections::VecDeque;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{
    Receiver, RecvTimeoutError, SyncSender, TryRecvError, TrySendError, sync_channel,
};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, TryLockError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use miniscent_game::FrameStats;
use mscent_platform::{Event, GpuWindow};
use mscent_render::{Framebuffer, View, VoxelChange};

use super::Mode;
use super::adaptive::Adaptive;
use super::highlight::Highlights;
pub use super::input::{Inbox, Input};
pub use super::slow::{Slow, set_slow_ms, slow_ms};
use crate::build::{Built, panic_text};
use crate::model::FlyCam;

/// One frame every 1/60 s.
const TARGET: Duration = Duration::from_micros(16_667);
/// How long before a Play frame is due the thread stops sleeping and spins.
const SPIN: Duration = Duration::from_millis(2);
/// Longest simulated step after a stall; the session sub-steps at 120 Hz.
const MAX_DT: Duration = Duration::from_millis(100);
/// Pending commands; beyond this a command is dropped, not queued, or held
/// back by the UI thread when it must not be lost.
const QUEUE: usize = 64;
/// What the thread draws and steps.
pub trait Scene: Send + 'static {
    /// Starts the game over (entering Play).
    fn restart(&mut self);
    /// Advances Play by `dt` with `events`; true when the game asks to quit.
    fn step(&mut self, dt: Duration, events: &[Event], stats: FrameStats) -> bool;
    /// Draws the picture `req` asks for.
    fn render(&mut self, req: &Request) -> Option<&Framebuffer>;
    /// Draws the same level with the light of `lit` from now on, leaving a
    /// running game as it is.
    fn relight(&mut self, _lit: Built) {}
    /// Changes voxel surfaces under the same brushes.
    fn apply_voxels(&mut self, _change: VoxelChange) {}
    /// Shows Play in `window` itself from now on, or (`None`) as frames for
    /// the pane again.
    fn set_surface(&mut self, _window: Option<Surface>) {}
    /// Whether the surface was given up since the last call, its frames being
    /// left to the pane.
    fn surface_lost(&mut self) -> bool {
        false
    }
}

/// The window Play can present into, the Play pane's own.
pub type Surface = Arc<dyn GpuWindow>;

/// A picture the thread is asked to draw.
pub struct Request {
    pub w: u32,
    pub h: u32,
    /// The pane's own size, which `w` and `h` scale down to hold the budget.
    pub pane: (u32, u32),
    /// Game view (true) or the editor's fly camera.
    pub play: bool,
    pub view: View,
    /// A note about the picture to write on it.
    pub banner: Option<&'static str>,
    /// Picked faces to tint and outline (editor view only).
    pub highlights: Highlights,
}

/// A finished picture, `rgba` bytes ready for the UI to show.
pub struct Frame {
    pub w: u32,
    pub h: u32,
    pub rgba: Vec<u8>,
    pub mode: Mode,
}

enum Cmd {
    Size(u32, u32),
    Mode(Mode),
    Camera(Box<FlyCam>),
    Banner(Option<&'static str>),
    Highlights(Highlights),
    Scene(Box<dyn Scene>),
    Relight(Box<Built>),
    /// One whole batch, so a frame never shows half a dab.
    Voxels(VoxelChange),
    Surface(Option<Surface>),
}

#[derive(Default)]
struct Shared {
    latest: Mutex<Option<Frame>>,
    camera: Mutex<Option<FlyCam>>,
    error: Mutex<Option<String>>,
    quit: AtomicBool,
    /// Input the UI handed over since the last frame took it.
    inbox: Inbox,
    surface_lost: AtomicBool,
    stop: AtomicBool,
    alive: AtomicBool,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Like `try_lock` but a poisoned lock still opens; `None` when busy.
fn try_lock<T>(m: &Mutex<T>) -> Option<MutexGuard<'_, T>> {
    match m.try_lock() {
        Ok(g) => Some(g),
        Err(TryLockError::Poisoned(p)) => Some(p.into_inner()),
        Err(TryLockError::WouldBlock) => None,
    }
}

/// The UI thread's end of a render thread.
pub struct Worker {
    tx: SyncSender<Cmd>,
    shared: Arc<Shared>,
    size: (u32, u32),
    input: Input,
    /// Commands that must not be lost, waiting for room in the queue.
    held: VecDeque<Cmd>,
}

impl Worker {
    /// Starts the thread on `scene` with the editor camera `fly`.
    pub fn spawn(scene: Box<dyn Scene>, fly: FlyCam) -> Result<(Worker, JoinHandle<()>), String> {
        let (tx, rx) = sync_channel(QUEUE);
        let shared = Arc::new(Shared::default());
        shared.alive.store(true, Ordering::Release);
        let sh = shared.clone();
        let handle = std::thread::Builder::new()
            .name("mscent-render".into())
            .spawn(move || {
                let r = catch_unwind(AssertUnwindSafe(|| run(scene, &rx, &sh, fly)));
                if let Err(p) = r {
                    *lock(&sh.error) = Some(panic_text(&*p));
                }
                sh.alive.store(false, Ordering::Release);
            })
            .map_err(|e| format!("cannot start the render thread: {e}"))?;
        let w = Worker {
            input: Input::new(shared.inbox.clone()),
            tx,
            shared,
            size: (0, 0),
            held: VecDeque::new(),
        };
        Ok((w, handle))
    }

    fn send(&self, cmd: Cmd) -> bool {
        !matches!(self.tx.try_send(cmd), Err(TrySendError::Full(_)))
    }

    /// Forwards input (see [`Input::send`]).
    pub fn input(&mut self, events: &[Event]) {
        self.input.send(events);
    }

    /// A way in for input of its own, which a pane hands over as it arrives.
    pub fn input_sender(&self) -> Input {
        Input::new(self.shared.inbox.clone())
    }

    /// Tells the thread the pane size; `(0, 0)` means "not visible".
    pub fn size(&mut self, w: u32, h: u32) {
        if self.size != (w, h) && self.send(Cmd::Size(w, h)) {
            self.size = (w, h);
        }
    }

    /// Changes mode. The input handed over so far belongs to the old one, and
    /// goes now, on this side: input sent after this returns is the new mode's,
    /// though the thread takes the command later. The thread holds the inbox
    /// only to empty it, so this does not wait on a frame.
    pub fn mode(&self, mode: Mode) {
        lock(&self.shared.inbox).clear();
        self.send(Cmd::Mode(mode));
    }

    pub fn highlights(&self, h: &Highlights) {
        self.send(Cmd::Highlights(h.clone()));
    }

    /// Sets the note on the picture; false when the queue had no room for it.
    pub fn banner(&self, banner: Option<&'static str>) -> bool {
        self.send(Cmd::Banner(banner))
    }

    /// Replaces the scene on the running thread.
    pub fn scene(&mut self, scene: Box<dyn Scene>) {
        self.held.push_back(Cmd::Scene(scene));
        self.flush();
    }

    /// Hands the current scene new light for the same level.
    pub fn relight(&mut self, lit: Built) {
        self.held.push_back(Cmd::Relight(Box::new(lit)));
        self.flush();
    }

    /// Has Play present into `window`, or (`None`) send frames again. Never
    /// lost, like a scene.
    pub fn surface(&mut self, window: Option<Surface>) {
        self.held.push_back(Cmd::Surface(window));
        self.flush();
    }

    /// Whether the scene gave its surface up since the last call.
    pub fn take_surface_lost(&self) -> bool {
        self.shared.surface_lost.swap(false, Ordering::AcqRel)
    }

    /// Changes voxel surfaces under the current scene. Never lost, like a scene.
    pub fn voxels(&mut self, change: VoxelChange) {
        self.held.push_back(Cmd::Voxels(change));
        self.flush();
    }

    /// Sends what was held back, in order, as far as the queue has room.
    pub fn flush(&mut self) {
        while let Some(cmd) = self.held.pop_front() {
            if let Err(TrySendError::Full(cmd)) = self.tx.try_send(cmd) {
                self.held.push_front(cmd);
                return;
            }
        }
    }

    pub fn camera(&self, fly: &FlyCam) {
        self.send(Cmd::Camera(Box::new(fly.clone())));
    }

    /// The newest finished frame, once. `None` when there is none or the
    /// thread holds the mailbox for its swap right now.
    pub fn take_frame(&self) -> Option<Frame> {
        try_lock(&self.shared.latest)?.take()
    }

    /// The thread's fly camera, when it is not mid-update.
    pub fn current_camera(&self) -> Option<FlyCam> {
        try_lock(&self.shared.camera)?.clone()
    }

    /// Whether the game asked to quit since the last call.
    pub fn take_quit(&self) -> bool {
        self.shared.quit.swap(false, Ordering::AcqRel)
    }

    /// The panic text, when the thread died.
    pub fn take_error(&self) -> Option<String> {
        try_lock(&self.shared.error)?.take()
    }

    pub fn alive(&self) -> bool {
        self.shared.alive.load(Ordering::Acquire)
    }

    /// Asks the thread to finish; it stops at its next frame boundary. The
    /// caller joins the handle later without blocking the UI.
    pub fn stop(self) {
        self.shared.stop.store(true, Ordering::Release);
    }
}

struct Loop {
    mode: Mode,
    size: (u32, u32),
    banner: Option<&'static str>,
    highlights: Highlights,
    dirty: bool,
    events: Vec<Event>,
    fly: FlyCam,
    /// Given to every scene, a new one too.
    surface: Option<Surface>,
}

impl Loop {
    fn apply(&mut self, cmd: Cmd, scene: &mut Box<dyn Scene>, sh: &Shared) {
        match cmd {
            Cmd::Size(w, h) => {
                self.size = (w, h);
                self.dirty = true;
            }
            Cmd::Mode(m) => {
                if m == Mode::Play && self.mode != Mode::Play {
                    scene.restart();
                }
                self.mode = m;
                // A quit asked for by a step that was already running when
                // the UI changed mode belongs to the old session.
                sh.quit.store(false, Ordering::Release);
                self.events.clear();
                self.dirty = true;
            }
            Cmd::Camera(c) => {
                self.fly = *c;
                *lock(&sh.camera) = Some(self.fly.clone());
                self.dirty = true;
            }
            Cmd::Banner(b) => {
                self.banner = b;
                self.dirty = true;
            }
            Cmd::Scene(s) => {
                *scene = s;
                scene.set_surface(self.surface.clone());
                self.dirty = true;
            }
            Cmd::Surface(window) => {
                self.surface.clone_from(&window);
                scene.set_surface(window);
                self.dirty = true;
            }
            Cmd::Relight(lit) => {
                scene.relight(*lit);
                self.dirty = true;
            }
            Cmd::Voxels(change) => {
                scene.apply_voxels(change);
                self.dirty = true;
            }
            Cmd::Highlights(h) => {
                self.highlights = h;
                self.dirty = true;
            }
        }
    }
}

fn run(mut scene: Box<dyn Scene>, rx: &Receiver<Cmd>, sh: &Shared, fly: FlyCam) {
    *lock(&sh.camera) = Some(fly.clone());
    let mut st = Loop {
        mode: Mode::Edit,
        size: (0, 0),
        banner: None,
        highlights: Highlights::default(),
        dirty: true,
        events: Vec::new(),
        fly,
        surface: None,
    };
    let mut adaptive = Adaptive { scale: 1.0 };
    let mut last = Instant::now();
    let mut cost = Duration::ZERO;
    while !sh.stop.load(Ordering::Acquire) {
        let now = Instant::now();
        let dt = now.duration_since(last).min(MAX_DT);
        last = now;
        loop {
            match rx.try_recv() {
                Ok(c) => st.apply(c, &mut scene, sh),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => return,
            }
        }
        st.events.append(&mut lock(&sh.inbox));
        advance(&mut st, &mut *scene, sh, dt, cost);
        let (w, h) = st.size;
        if st.dirty && w > 0 && h > 0 {
            let t = Instant::now();
            let (iw, ih) = adaptive.size(w, h);
            publish(&st, &mut *scene, sh, (iw, ih));
            cost = t.elapsed();
            adaptive.update(cost);
            st.dirty = false;
        }
        // Sleep to the next frame, still taking commands as they arrive. Play
        // spins the last moments as the game does: a sleep can overshoot by a
        // millisecond, which costs a 60 Hz frame now and then.
        let deadline = now + TARGET;
        let spin = if st.mode == Mode::Play && st.size.0 > 0 {
            SPIN
        } else {
            Duration::ZERO
        };
        while let Some(left) = deadline.checked_duration_since(Instant::now() + spin) {
            match rx.recv_timeout(left) {
                Ok(c) => st.apply(c, &mut scene, sh),
                Err(RecvTimeoutError::Timeout) => break,
                Err(RecvTimeoutError::Disconnected) => return,
            }
        }
        while Instant::now() < deadline {
            std::hint::spin_loop();
        }
    }
}

fn advance(st: &mut Loop, scene: &mut dyn Scene, sh: &Shared, dt: Duration, cost: Duration) {
    let mut events = std::mem::take(&mut st.events);
    match st.mode {
        Mode::Edit => {
            let mut moved = false;
            for e in &events {
                moved |= st.fly.event(e);
            }
            moved |= st.fly.step(dt.as_secs_f32());
            if moved {
                st.dirty = true;
                *lock(&sh.camera) = Some(st.fly.clone());
            }
        }
        Mode::Play => {
            let secs = dt.as_secs_f32();
            let stats = FrameStats {
                fps: if secs > 0.0 { 1.0 / secs } else { 0.0 },
                frame_ms: cost.as_secs_f32() * 1000.0,
            };
            if scene.step(dt, &events, stats) {
                sh.quit.store(true, Ordering::Release);
            }
            st.dirty = true;
        }
    }
    // Hand the emptied buffer back so it keeps its allocation.
    events.clear();
    st.events = events;
}

/// A straight loop over fixed-size pixels, which the compiler can vectorize: at
/// 4K this runs eight million times a frame.
fn rgba_of(pixels: &[u32]) -> Vec<u8> {
    let mut rgba = vec![255u8; pixels.len() * 4];
    for (out, p) in rgba.as_chunks_mut::<4>().0.iter_mut().zip(pixels) {
        *out = [(p >> 16) as u8, (p >> 8) as u8, *p as u8, 255];
    }
    rgba
}

fn publish(st: &Loop, scene: &mut dyn Scene, sh: &Shared, (w, h): (u32, u32)) {
    let req = Request {
        w,
        h,
        pane: st.size,
        play: st.mode == Mode::Play,
        view: st.fly.view(),
        banner: st.banner,
        highlights: st.highlights.clone(),
    };
    // A frame Play showed in its own window is not one for the pane.
    let frame = scene.render(&req).map(|fb| Frame {
        w: fb.width(),
        h: fb.height(),
        rgba: rgba_of(fb.pixels()),
        mode: st.mode,
    });
    if scene.surface_lost() {
        sh.surface_lost.store(true, Ordering::Release);
    }
    let Some(frame) = frame else { return };
    *lock(&sh.latest) = Some(frame);
}
