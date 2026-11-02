//! The thread that evaluates pages. The UI thread never waits for it: requests go
//! into a one-slot mailbox with `try_lock`, a stale run is cancelled through an
//! atomic, and what it finishes waits in a one-slot outbox read with `try_lock`.
//! Both locks are held for moments only, never across an evaluation. A new request
//! replaces one not yet started, and a new picture one not yet shown. A panic in a
//! kernel is caught and reported as an error text.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError, TryLockError};
use std::thread::JoinHandle;
use std::time::Duration;

use mscent_render::Texture;
use mscent_texgen::Recipe;
use mscent_texgen::graph::{Cache, Graph};

use super::eval::{self, Failure, Output, Picture, Request, Thumbs};
use super::lit::LitTile;
use crate::build::panic_text;

/// Results kept between runs, so that a change re-runs only what depends on it.
const CACHE_BYTES: usize = 256 * 1024 * 1024;
/// Time between frames of the lit view: 25 a second.
const ANIMATION: Duration = Duration::from_millis(40);

/// A material to make, for Apply and Save. It carries its page: edits made while
/// it runs are not part of what was asked for.
pub struct BakeJob {
    pub name: String,
    pub recipe: Recipe,
    pub graph: Graph,
}

/// A finished [`BakeJob`].
pub struct Baked {
    pub name: String,
    pub recipe: Recipe,
    pub texture: Result<Texture, String>,
}

#[derive(Default)]
struct Inbox {
    request: Option<Request>,
    bake: Option<BakeJob>,
    /// Stop animating: the workspace is not on screen.
    idle: bool,
    stop: bool,
}

/// What the thread has finished and the UI has not taken yet.
#[derive(Default)]
pub struct Outbox {
    pub picture: Option<Picture>,
    pub thumbs: Option<Thumbs>,
    pub failure: Option<Failure>,
    pub baked: Option<Baked>,
}

impl Outbox {
    fn put(&mut self, output: Output) {
        match output {
            Output::Picture(p) => self.picture = Some(p),
            Output::Thumbs(t) => self.thumbs = Some(t),
            Output::Failure(f) => self.failure = Some(f),
        }
    }
}

#[derive(Default)]
struct Shared {
    inbox: Mutex<Inbox>,
    wake: Condvar,
    outbox: Mutex<Outbox>,
    /// Set whenever the inbox gets something newer than the run in progress.
    cancel: AtomicBool,
    /// Holds every evaluation up while a test has it.
    #[cfg(test)]
    gate: Mutex<()>,
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

enum Work {
    Stop,
    Idle,
    Evaluate(Box<Request>),
    Bake(Box<BakeJob>),
    Animate,
}

/// Waits for something to do; with `animating`, no longer than one frame.
fn next_work(shared: &Shared, animating: bool) -> Work {
    let mut inbox = lock(&shared.inbox);
    loop {
        if inbox.stop {
            return Work::Stop;
        }
        if let Some(job) = inbox.bake.take() {
            return Work::Bake(Box::new(job));
        }
        // Reset under the lock that set it: what is taken here is the newest, and
        // anything newer sets the flag again after this.
        if let Some(request) = inbox.request.take() {
            shared.cancel.store(false, Ordering::Relaxed);
            return Work::Evaluate(Box::new(request));
        }
        if std::mem::take(&mut inbox.idle) {
            shared.cancel.store(false, Ordering::Relaxed);
            return Work::Idle;
        }
        if animating {
            let (guard, wait) = shared
                .wake
                .wait_timeout(inbox, ANIMATION)
                .unwrap_or_else(PoisonError::into_inner);
            inbox = guard;
            if wait.timed_out() {
                return Work::Animate;
            }
        } else {
            inbox = shared
                .wake
                .wait(inbox)
                .unwrap_or_else(PoisonError::into_inner);
        }
    }
}

/// Evaluates inside `catch_unwind`. A panic leaves the cache of unknown worth, so
/// it starts over.
fn guarded_evaluate(shared: &Shared, request: &Request, cache: &mut Cache) -> Option<LitTile> {
    #[cfg(test)]
    let _gate = lock(&shared.gate);
    let run = catch_unwind(AssertUnwindSafe(|| {
        let mut send = |output: Output| lock(&shared.outbox).put(output);
        eval::evaluate(request, cache, &shared.cancel, &mut send)
    }));
    run.unwrap_or_else(|panic| {
        *cache = Cache::new(CACHE_BYTES);
        lock(&shared.outbox).failure = Some(Failure {
            seq: request.seq,
            why: format!("an operator failed: {}", panic_text(&*panic)),
            block: None,
        });
        None
    })
}

fn guarded_bake(job: &BakeJob, cache: &mut Cache) -> Result<Texture, String> {
    catch_unwind(AssertUnwindSafe(|| {
        eval::bake(&job.recipe, &job.graph, cache)
    }))
    .unwrap_or_else(|panic| {
        *cache = Cache::new(CACHE_BYTES);
        Err(format!("an operator failed: {}", panic_text(&*panic)))
    })
}

fn thread_main(shared: &Shared) {
    let mut cache = Cache::new(CACHE_BYTES);
    let mut tile: Option<LitTile> = None;
    let mut step = 0;
    loop {
        let animating = tile.as_ref().is_some_and(LitTile::animates);
        match next_work(shared, animating) {
            Work::Stop => return,
            Work::Idle => tile = None,
            Work::Evaluate(request) => {
                tile = guarded_evaluate(shared, &request, &mut cache);
            }
            Work::Bake(job) => {
                let texture = guarded_bake(&job, &mut cache);
                let job = *job;
                lock(&shared.outbox).baked = Some(Baked {
                    name: job.name,
                    recipe: job.recipe,
                    texture,
                });
            }
            Work::Animate => {
                step = (step + 1) % 1024;
                if let Some(t) = &tile {
                    lock(&shared.outbox).picture = Some(t.render(step));
                }
            }
        }
    }
}

/// The UI thread's end of the thread.
pub struct Worker {
    shared: Arc<Shared>,
    handle: Option<JoinHandle<()>>,
}

impl Worker {
    pub fn spawn() -> Result<Worker, String> {
        let shared = Arc::new(Shared::default());
        let theirs = shared.clone();
        let handle = std::thread::Builder::new()
            .name("mscent-texstudio".into())
            .spawn(move || thread_main(&theirs))
            .map_err(|e| format!("cannot start the texture thread: {e}"))?;
        Ok(Worker {
            shared,
            handle: Some(handle),
        })
    }

    /// Asks for `request`, cancelling what is running; gives it back when the
    /// mailbox is busy this moment, to try again on the next tick.
    pub fn post(&self, request: Request) -> Result<(), Box<Request>> {
        let Some(mut inbox) = try_lock(&self.shared.inbox) else {
            return Err(Box::new(request));
        };
        inbox.request = Some(request);
        self.shared.cancel.store(true, Ordering::Relaxed);
        self.shared.wake.notify_one();
        Ok(())
    }

    /// Asks for a material; gives it back when the mailbox is busy this moment.
    pub fn post_bake(&self, job: BakeJob) -> Result<(), Box<BakeJob>> {
        let Some(mut inbox) = try_lock(&self.shared.inbox) else {
            return Err(Box::new(job));
        };
        inbox.bake = Some(job);
        self.shared.wake.notify_one();
        Ok(())
    }

    /// Stops whatever is running and any animation; false when busy this moment.
    pub fn post_idle(&self) -> bool {
        let Some(mut inbox) = try_lock(&self.shared.inbox) else {
            return false;
        };
        inbox.request = None;
        inbox.idle = true;
        self.shared.cancel.store(true, Ordering::Relaxed);
        self.shared.wake.notify_one();
        true
    }

    /// What has finished since the last look; `None` when busy this moment.
    pub fn take(&self) -> Option<Outbox> {
        try_lock(&self.shared.outbox).map(|mut outbox| std::mem::take(&mut *outbox))
    }

    /// Whether the thread has ended, which only a bug outside a run can make it.
    pub fn died(&self) -> bool {
        self.handle.as_ref().is_some_and(JoinHandle::is_finished)
    }

    /// A handle on the thread's locks for a test to hold.
    #[cfg(test)]
    pub fn locks(&self) -> Locks {
        Locks(self.shared.clone())
    }
}

impl Drop for Worker {
    /// Stops the thread and waits for it, which at the end of a run is brief: the
    /// run in progress is cancelled at its next block.
    fn drop(&mut self) {
        lock(&self.shared.inbox).stop = true;
        self.shared.cancel.store(true, Ordering::Relaxed);
        self.shared.wake.notify_all();
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

/// What a tick could wait for, and what holds an evaluation up.
#[cfg(test)]
pub struct Locks(Arc<Shared>);

#[cfg(test)]
impl Locks {
    /// Both locks a tick touches, held until the guards go.
    pub fn mailboxes(&self) -> impl Sized + '_ {
        (lock(&self.0.inbox), lock(&self.0.outbox))
    }

    /// Holds every evaluation up for as long as the guard lives.
    pub fn gate(&self) -> MutexGuard<'_, ()> {
        lock(&self.0.gate)
    }
}
