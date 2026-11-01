use super::*;
use std::sync::Barrier;
use std::sync::atomic::AtomicU32;

fn counts(n: usize) -> Vec<AtomicU32> {
    (0..n).map(|_| AtomicU32::new(0)).collect()
}

#[test]
fn one_worker_makes_no_thread_and_runs_on_the_caller() {
    for workers in [0, 1] {
        let mut pool = JobPool::new(workers);
        assert_eq!(pool.helpers(), 0);
        let me = thread::current().id();
        let ran = AtomicU32::new(0);
        pool.run(50, &|_| {
            assert_eq!(thread::current().id(), me);
            ran.fetch_add(1, Ordering::Relaxed);
        });
        assert_eq!(ran.load(Ordering::Relaxed), 50);
    }
}

#[test]
fn every_job_runs_exactly_once_over_many_calls() {
    let mut pool = JobPool::new(4);
    assert_eq!(pool.helpers(), 3);
    // Fewer jobs than workers, as many, more, and repeated calls so a helper
    // still going back to sleep meets the next one.
    for jobs in [0, 1, 2, 3, 4, 5, 64, 1000] {
        for _ in 0..50 {
            let seen = counts(jobs);
            pool.run(jobs, &|i| {
                seen[i].fetch_add(1, Ordering::Relaxed);
            });
            assert!(seen.iter().all(|c| c.load(Ordering::Relaxed) == 1));
        }
    }
}

#[test]
fn jobs_really_run_at_the_same_time() {
    // Each job waits for all the others: this can only finish on three threads.
    let mut pool = JobPool::new(3);
    let barrier = Barrier::new(3);
    pool.run(3, &|_| {
        barrier.wait();
    });
}

#[test]
fn a_panic_reaches_the_caller_and_the_pool_still_works() {
    let mut pool = JobPool::new(4);
    for _ in 0..20 {
        let result = panic::catch_unwind(AssertUnwindSafe(|| {
            pool.run(32, &|i| {
                assert!(i != 7, "job seven");
            });
        }));
        let payload = result.expect_err("the panic must surface");
        let text = payload.downcast_ref::<&str>().copied().unwrap_or("");
        assert!(text.contains("job seven"), "{text:?}");
        let ran = AtomicU32::new(0);
        pool.run(32, &|_| {
            ran.fetch_add(1, Ordering::Relaxed);
        });
        assert_eq!(ran.load(Ordering::Relaxed), 32);
    }
}

/// A panic payload that panics again when dropped.
struct DropBomb;

impl Drop for DropBomb {
    fn drop(&mut self) {
        panic!("payload dropped");
    }
}

#[test]
fn a_payload_that_panics_when_dropped_cannot_break_out_of_the_work() {
    let mut pool = JobPool::new(4);
    // The barrier makes all four jobs panic, so all but one payload is spare.
    let barrier = Barrier::new(4);
    let result = panic::catch_unwind(AssertUnwindSafe(|| {
        pool.run(4, &|_| {
            barrier.wait();
            panic::panic_any(DropBomb);
        });
    }));
    core::mem::forget(result.expect_err("the panic must surface"));
    let ran = AtomicU32::new(0);
    pool.run(8, &|_| {
        ran.fetch_add(1, Ordering::Relaxed);
    });
    assert_eq!(ran.load(Ordering::Relaxed), 8);
}

#[test]
fn a_panic_on_one_worker_also_reaches_the_caller() {
    let mut pool = JobPool::new(1);
    let result = panic::catch_unwind(AssertUnwindSafe(|| {
        pool.run(3, &|_| panic!("alone"));
    }));
    assert!(result.is_err());
}

#[test]
fn dropping_a_pool_ends_its_threads() {
    for workers in [1, 2, 8] {
        let mut pool = JobPool::new(workers);
        pool.run(4, &|_| {});
        drop(pool);
    }
    // Never used at all, so the helpers may not even have started waiting.
    drop(JobPool::new(8));
}
