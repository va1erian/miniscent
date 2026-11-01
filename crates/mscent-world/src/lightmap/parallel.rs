//! Tiny work-sharing helper on `std::thread::scope`.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;

/// Runs `f(0..count)` on up to `workers` threads and returns the results in
/// index order. Work is handed out one index at a time so uneven jobs balance.
pub fn par_map<T: Send>(workers: usize, count: usize, f: impl Fn(usize) -> T + Sync) -> Vec<T> {
    let workers = workers.clamp(1, count.max(1));
    if workers == 1 {
        return (0..count).map(f).collect();
    }
    let next = AtomicUsize::new(0);
    let mut done: Vec<(usize, T)> = Vec::with_capacity(count);
    thread::scope(|s| {
        let handles: Vec<_> = (0..workers)
            .map(|_| {
                s.spawn(|| {
                    let mut out = Vec::new();
                    loop {
                        let i = next.fetch_add(1, Ordering::Relaxed);
                        if i >= count {
                            break out;
                        }
                        out.push((i, f(i)));
                    }
                })
            })
            .collect();
        for h in handles {
            match h.join() {
                Ok(part) => done.extend(part),
                // A worker panic is a bug in `f`; surface it unchanged.
                Err(payload) => std::panic::resume_unwind(payload),
            }
        }
    });
    done.sort_by_key(|(i, _)| *i);
    done.into_iter().map(|(_, t)| t).collect()
}
