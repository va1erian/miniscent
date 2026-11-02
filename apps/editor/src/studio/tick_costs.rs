//! What UI ticks cost, for tests. A wall-clock sample measures the scheduler as
//! much as the tick, so on a busy machine a single worst case proves nothing.
//! This only answers whether ticks are cheap as a rule; that they never wait is
//! proved without a clock, by holding up what they could wait for
//! (`cache_lock_tests.rs`, and the gated scene in `tests.rs`).

use std::time::{Duration, Instant};

/// One frame of the UI's timer: what a tick may never cost as a rule.
const FRAME: Duration = Duration::from_millis(16);

#[derive(Default)]
pub struct TickCosts(Vec<Duration>);

impl TickCosts {
    /// Runs `tick` and records how long it took.
    pub fn timed<T>(&mut self, tick: impl FnOnce() -> T) -> T {
        let t = Instant::now();
        let out = tick();
        self.0.push(t.elapsed());
        out
    }

    /// Adds the samples of `other`.
    pub fn merged(mut self, other: &TickCosts) -> TickCosts {
        self.0.extend_from_slice(&other.0);
        self
    }

    pub fn worst(&self) -> Duration {
        self.0.iter().copied().max().unwrap_or_default()
    }

    /// The cost nine ticks in ten stay under (nearest rank).
    pub fn p90(&self) -> Duration {
        let mut sorted = self.0.clone();
        sorted.sort_unstable();
        let rank = (sorted.len() * 9).div_ceil(10);
        rank.checked_sub(1)
            .and_then(|i| sorted.get(i).copied())
            .unwrap_or_default()
    }

    /// Fails when ticks are not cheap as a rule; a few descheduled ones pass.
    #[track_caller]
    pub fn assert_cheap(&self) {
        assert!(
            self.p90() < FRAME,
            "ticks take {:?} at the 90th percentile of {} (worst {:?})",
            self.p90(),
            self.0.len(),
            self.worst()
        );
    }
}

#[test]
fn p90_ignores_the_odd_descheduled_tick_but_not_a_slow_rule() {
    let ms = |list: &[u64]| TickCosts(list.iter().map(|m| Duration::from_millis(*m)).collect());
    let noisy = ms(&[1, 1, 1, 1, 1, 1, 1, 1, 1, 90]);
    assert_eq!(noisy.p90(), Duration::from_millis(1));
    assert_eq!(noisy.worst(), Duration::from_millis(90));
    noisy.assert_cheap();
    let slow = ms(&[1, 1, 1, 1, 1, 1, 1, 1, 90, 90]);
    assert_eq!(slow.p90(), Duration::from_millis(90));
    assert_eq!(TickCosts::default().p90(), Duration::ZERO);
}
