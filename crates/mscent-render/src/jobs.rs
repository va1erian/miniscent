//! A pool of parked threads that runs one parallel-for at a time.
//!
//! Spawning a frame's workers costs several times what waking parked ones does
//! (see `docs/particles.md`), so the threads are made once and sleep between
//! calls. [`JobPool::run`] is the whole interface: there is no task graph, no
//! priority and no queue of pending work, because a frame only ever needs
//! "these N jobs, now, and return when they are done".
//!
//! One worker is a valid setting: it makes no thread and runs everything on the
//! caller, for a platform where threads are missing or do not help.

// The crate denies unsafe code; this module allows it for the one block in
// `run` that hands a borrowed closure to threads that outlive the borrow.
#![allow(unsafe_code)]

use std::any::Any;
use std::panic::{self, AssertUnwindSafe};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::thread::{self, JoinHandle};

/// A running [`JobPool::run`], with its borrows erased. Only valid until that
/// call returns, which waits for every helper to drop it.
type Work = &'static (dyn Fn() + Sync);

#[derive(Default)]
struct State {
    work: Option<Work>,
    /// Helpers that may still join the current work.
    slots: usize,
    /// Helpers inside the current work.
    busy: usize,
    quit: bool,
}

#[derive(Default)]
struct Shared {
    state: Mutex<State>,
    wake: Condvar,
    idle: Condvar,
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// Runs parallel-for calls on `workers - 1` parked threads and the caller.
pub struct JobPool {
    shared: Arc<Shared>,
    threads: Vec<JoinHandle<()>>,
}

impl core::fmt::Debug for JobPool {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("JobPool")
            .field("helpers", &self.threads.len())
            .finish()
    }
}

impl JobPool {
    /// A pool for `workers` workers, the caller being one of them. A thread
    /// the system refuses is simply a helper fewer, so this cannot fail.
    pub fn new(workers: usize) -> Self {
        let shared = Arc::new(Shared::default());
        let threads = (1..workers)
            .map_while(|_| {
                let shared = Arc::clone(&shared);
                thread::Builder::new()
                    .name("mscent-worker".into())
                    .spawn(move || helper_loop(&shared))
                    .ok()
            })
            .collect();
        Self { shared, threads }
    }

    /// Threads this pool made; the caller works alongside them.
    pub fn helpers(&self) -> usize {
        self.threads.len()
    }

    /// Runs `f(0)` to `f(jobs - 1)`, each exactly once unless a job panics,
    /// and returns when all are done. The first panic stops jobs not yet
    /// started and continues on the caller, once no thread is using `f`.
    pub fn run<F: Fn(usize) + Sync>(&mut self, jobs: usize, f: &F) {
        let helpers = self.threads.len().min(jobs.saturating_sub(1));
        if helpers == 0 {
            (0..jobs).for_each(f);
            return;
        }
        let next = AtomicUsize::new(0);
        let panicked: Mutex<Option<Box<dyn Any + Send>>> = Mutex::new(None);
        let work = || {
            loop {
                let i = next.fetch_add(1, Ordering::Relaxed);
                if i >= jobs {
                    return;
                }
                if let Err(p) = panic::catch_unwind(AssertUnwindSafe(|| f(i))) {
                    let mut first = panicked.lock().unwrap_or_else(PoisonError::into_inner);
                    if first.is_none() {
                        *first = Some(p);
                    } else {
                        // Dropping a payload runs its code, which may panic,
                        // and nothing may unwind out of `work`.
                        core::mem::forget(p);
                    }
                    next.store(jobs, Ordering::Relaxed);
                }
            }
        };
        let borrowed: &(dyn Fn() + Sync) = &work;
        // SAFETY: this only lengthens the lifetime in the type. `work` and what
        // it borrows live until the end of this function, and the wait below
        // does not finish until `slots` is 0 and `busy` is 0, so no helper can
        // still hold or take this reference by then; `work` is cleared under
        // the same lock. Nothing between here and that wait can unwind, since
        // `work` catches panics from `f`. `&mut self` rules out a second `run`
        // overwriting it.
        let erased: Work = unsafe { core::mem::transmute(borrowed) };
        {
            let mut s = self.shared.lock();
            s.work = Some(erased);
            s.slots = helpers;
        }
        for _ in 0..helpers {
            self.shared.wake.notify_one();
        }
        work();
        self.finish();
        let first = panicked.into_inner();
        if let Some(p) = first.unwrap_or_else(PoisonError::into_inner) {
            panic::resume_unwind(p);
        }
    }

    /// Takes back slots no helper woke up for and waits out the rest: the jobs
    /// have all been claimed, so a late helper has nothing to do.
    fn finish(&self) {
        let mut s = self.shared.lock();
        s.slots = 0;
        while s.busy > 0 {
            s = self
                .shared
                .idle
                .wait(s)
                .unwrap_or_else(PoisonError::into_inner);
        }
        s.work = None;
    }
}

impl Drop for JobPool {
    fn drop(&mut self) {
        self.shared.lock().quit = true;
        self.shared.wake.notify_all();
        for t in self.threads.drain(..) {
            // A helper's panics are caught inside the work, so join cannot
            // report anything worth a panic during drop.
            let _ = t.join();
        }
    }
}

fn helper_loop(shared: &Shared) {
    let mut s = shared.lock();
    while !s.quit {
        match s.work.filter(|_| s.slots > 0) {
            Some(work) => {
                s.slots -= 1;
                s.busy += 1;
                drop(s);
                work();
                s = shared.lock();
                s.busy -= 1;
                if s.busy == 0 {
                    shared.idle.notify_all();
                }
            }
            None => s = shared.wake.wait(s).unwrap_or_else(PoisonError::into_inner),
        }
    }
}

#[cfg(test)]
#[path = "jobs_tests.rs"]
mod jobs_tests;
