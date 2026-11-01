//! The render thread. It owns a [`Scene`] (the built level and its game
//! session), steps and renders it at a 60 Hz target and publishes the newest
//! frame. The UI thread never waits for it: input goes in through a bounded
//! channel with `try_send`, frames come out through a one-slot mailbox read
//! with `try_lock`, and a slow scene costs resolution (the adaptive scale),
//! not UI responsiveness. Stale frames are replaced in the mailbox, never
//! queued. A panic in the scene is caught and reported as an error text. A new
//! scene replaces the old one on the running thread, so the fly camera and any
//! held key carry straight over an edit.

use std::collections::VecDeque;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{
    Receiver, RecvTimeoutError, SyncSender, TryRecvError, TrySendError, sync_channel,
};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, TryLockError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use miniscent_game::FrameStats;
use mscent_platform::Event;
use mscent_render::{Framebuffer, View};

use super::Mode;
use super::highlight::Highlights;
use crate::build::{Built, panic_text};
use crate::model::FlyCam;

/// One frame every 1/60 s.
const TARGET: Duration = Duration::from_micros(16_667);
/// Longest simulated step after a stall; the session sub-steps at 120 Hz.
const MAX_DT: Duration = Duration::from_millis(100);
/// Pending command batches; beyond this input is dropped, not queued.
const QUEUE: usize = 64;
/// Extra delay per rendered frame, in ms (the hidden `--slow-play-ms`).
static SLOW_MS: AtomicU64 = AtomicU64::new(0);

/// Makes every scene created from now on take `ms` longer per frame, to
/// reproduce a slow machine.
pub fn set_slow_ms(ms: u64) {
    SLOW_MS.store(ms, Ordering::Relaxed);
}

pub fn slow_ms() -> u64 {
    SLOW_MS.load(Ordering::Relaxed)
}

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
}

/// A picture the thread is asked to draw.
pub struct Request {
    pub w: u32,
    pub h: u32,
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

/// Delays every frame, wrapping a scene (the `--slow-play-ms` flag).
pub struct Slow {
    pub inner: Box<dyn Scene>,
    pub delay: Duration,
}

impl Scene for Slow {
    fn restart(&mut self) {
        self.inner.restart();
    }

    fn step(&mut self, dt: Duration, events: &[Event], stats: FrameStats) -> bool {
        self.inner.step(dt, events, stats)
    }

    fn render(&mut self, req: &Request) -> Option<&Framebuffer> {
        std::thread::sleep(self.delay);
        self.inner.render(req)
    }

    fn relight(&mut self, lit: Built) {
        self.inner.relight(lit);
    }
}

enum Cmd {
    Input(Vec<Event>),
    Size(u32, u32),
    Mode(Mode),
    Camera(Box<FlyCam>),
    Banner(Option<&'static str>),
    Highlights(Highlights),
    Scene(Box<dyn Scene>),
    Relight(Box<Built>),
}

#[derive(Default)]
struct Shared {
    latest: Mutex<Option<Frame>>,
    camera: Mutex<Option<FlyCam>>,
    error: Mutex<Option<String>>,
    quit: AtomicBool,
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
    lost_input: bool,
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
            tx,
            shared,
            size: (0, 0),
            lost_input: false,
            held: VecDeque::new(),
        };
        Ok((w, handle))
    }

    fn send(&self, cmd: Cmd) -> bool {
        !matches!(self.tx.try_send(cmd), Err(TrySendError::Full(_)))
    }

    /// Forwards input. When the thread is so far behind that the queue is
    /// full the batch is dropped and the next one starts with a focus-lost
    /// event, so no key stays held.
    pub fn input(&mut self, events: &[Event]) {
        if events.is_empty() && !self.lost_input {
            return;
        }
        let mut batch = Vec::with_capacity(events.len() + 1);
        if self.lost_input {
            batch.push(Event::FocusLost);
        }
        batch.extend_from_slice(events);
        self.lost_input = !self.send(Cmd::Input(batch));
    }

    /// Tells the thread the pane size; `(0, 0)` means "not visible".
    pub fn size(&mut self, w: u32, h: u32) {
        if self.size != (w, h) && self.send(Cmd::Size(w, h)) {
            self.size = (w, h);
        }
    }

    pub fn mode(&self, mode: Mode) {
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

/// Scales the internal resolution to keep the frame cost near the budget.
struct Adaptive {
    scale: f32,
}

impl Adaptive {
    fn update(&mut self, cost: Duration) {
        let ms = cost.as_secs_f32() * 1000.0;
        if ms > 12.0 {
            self.scale = (self.scale * 0.85).max(0.25);
        } else if ms < 7.0 {
            self.scale = (self.scale * 1.1).min(1.0);
        }
    }

    fn size(&self, w: u32, h: u32) -> (u32, u32) {
        let s = |v: u32| ((v as f32 * self.scale) as u32).clamp(16, v.max(16));
        (s(w), s(h))
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
}

impl Loop {
    fn apply(&mut self, cmd: Cmd, scene: &mut Box<dyn Scene>, sh: &Shared) {
        match cmd {
            Cmd::Input(mut e) => self.events.append(&mut e),
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
                self.dirty = true;
            }
            Cmd::Relight(lit) => {
                scene.relight(*lit);
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
        // Sleep to the next frame, still taking commands as they arrive.
        let deadline = now + TARGET;
        while let Some(left) = deadline.checked_duration_since(Instant::now()) {
            match rx.recv_timeout(left) {
                Ok(c) => st.apply(c, &mut scene, sh),
                Err(RecvTimeoutError::Timeout) => break,
                Err(RecvTimeoutError::Disconnected) => return,
            }
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

fn publish(st: &Loop, scene: &mut dyn Scene, sh: &Shared, (w, h): (u32, u32)) {
    let req = Request {
        w,
        h,
        play: st.mode == Mode::Play,
        view: st.fly.view(),
        banner: st.banner,
        highlights: st.highlights.clone(),
    };
    let Some(fb) = scene.render(&req) else { return };
    let mut rgba = Vec::with_capacity(fb.pixels().len() * 4);
    for p in fb.pixels() {
        rgba.extend_from_slice(&[(p >> 16) as u8, (p >> 8) as u8, *p as u8, 255]);
    }
    let frame = Frame {
        w: fb.width(),
        h: fb.height(),
        rgba,
        mode: st.mode,
    };
    *lock(&sh.latest) = Some(frame);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adaptive_scale_drops_when_slow_and_recovers_within_bounds() {
        let mut a = Adaptive { scale: 1.0 };
        for _ in 0..30 {
            a.update(Duration::from_millis(100));
        }
        assert_eq!(a.scale, 0.25);
        assert_eq!(a.size(960, 600), (240, 150));
        for _ in 0..60 {
            a.update(Duration::from_millis(2));
        }
        assert_eq!(a.scale, 1.0);
        assert_eq!(a.size(10, 10), (16, 16));
    }
}
