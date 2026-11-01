#![forbid(unsafe_code)]

//! [`MonotonicClock`]: the real-time [`Clock`] for a native backend.

use std::time::{Duration, Instant};

use xui_core::motion::Clock;

/// A [`Clock`] over [`std::time::Instant`], reading the time since it was
/// created.
///
/// The portable `xui-core::motion` module never reads a clock itself; a
/// driver on a desktop gets real time from this, and a test or a headless run
/// uses [`ManualClock`](xui_core::motion::ManualClock) instead. `Instant` is
/// monotonic, so successive readings never go backwards.
///
/// # Not for `wasm32-unknown-unknown`
///
/// [`Instant::now`] panics on `wasm32-unknown-unknown`, which has no system
/// clock, so creating or reading this clock there panics. A web build reads the
/// browser's clock itself (`performance.now()`) and implements
/// [`Clock`](xui_core::motion::Clock) over that.
#[derive(Clone, Copy, Debug)]
pub struct MonotonicClock {
    origin: Instant,
}

impl MonotonicClock {
    /// A clock whose origin is now: [`Clock::now`] reads the time since this
    /// call.
    pub fn new() -> MonotonicClock {
        MonotonicClock {
            origin: Instant::now(),
        }
    }
}

impl Default for MonotonicClock {
    fn default() -> MonotonicClock {
        MonotonicClock::new()
    }
}

impl Clock for MonotonicClock {
    fn now(&self) -> Duration {
        self.origin.elapsed()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn readings_never_go_backwards() {
        let clock = MonotonicClock::new();
        let mut previous = clock.now();
        for _ in 0..1000 {
            let reading = clock.now();
            assert!(reading >= previous);
            previous = reading;
        }
    }

    #[test]
    fn the_origin_is_the_moment_of_creation() {
        let before = Instant::now();
        let clock = MonotonicClock::new();
        // Read back at once, the clock cannot be ahead of the wall time that
        // has passed since just before it was created.
        assert!(clock.now() <= before.elapsed());
    }

    #[test]
    fn it_is_usable_as_a_trait_object() {
        let clock: Box<dyn Clock> = Box::new(MonotonicClock::default());
        assert!(clock.now() < Duration::from_secs(60));
    }
}
