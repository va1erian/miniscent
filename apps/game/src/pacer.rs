//! Holds frames to a steady rate, so a high cap feels even and not only fast.

use std::time::{Duration, Instant};

/// Each frame is due one period after the last one was *due*, not after it
/// ended, so an early finish or a late wake does not shift the rhythm. A frame
/// that overruns its deadline starts the schedule over from now: the frames
/// after it keep their full length and do not rush to catch up, which would be
/// seen as a run of short frames.
pub struct Pacer {
    period: Duration,
    due: Instant,
    /// Slept short of the deadline, since the OS sleep is coarse; the rest
    /// spins. A quarter of the period at most, so a high cap does not spend
    /// half of every frame spinning.
    spin: Duration,
}

impl Pacer {
    pub fn new(fps: f32) -> Self {
        let period = Duration::from_secs_f32(1.0 / fps.clamp(1.0, 1000.0));
        Self {
            period,
            due: Instant::now() + period,
            spin: (period / 4).min(Duration::from_millis(2)),
        }
    }

    /// The deadline after the one due at `due`, given that it is `now`.
    fn next_due(&self, now: Instant) -> Instant {
        let next = self.due + self.period;
        if now > next { now + self.period } else { next }
    }

    /// Blocks until the current frame's deadline, then schedules the next.
    pub fn wait(&mut self) {
        if let Some(left) = self.due.checked_duration_since(Instant::now())
            && left > self.spin
        {
            std::thread::sleep(left - self.spin);
        }
        while Instant::now() < self.due {
            std::hint::spin_loop();
        }
        self.due = self.next_due(Instant::now());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pacer_keeps_its_rhythm_through_jitter_and_resets_after_an_overrun() {
        let p = Pacer::new(240.0);
        let start = p.due;
        let ms = Duration::from_millis;
        // A frame that woke 1 ms late does not push the next deadline back.
        assert_eq!(p.next_due(start + ms(1)), start + p.period);
        // An overrun starts the schedule over instead of bursting to catch up.
        let late = start + p.period + ms(1);
        assert_eq!(p.next_due(late), late + p.period);
        let stalled = start + ms(500);
        assert_eq!(p.next_due(stalled), stalled + p.period);
    }

    #[test]
    fn the_spin_margin_is_a_quarter_of_the_period_at_most() {
        assert_eq!(Pacer::new(240.0).spin, Pacer::new(240.0).period / 4);
        assert_eq!(Pacer::new(60.0).spin, Duration::from_millis(2));
    }

    #[test]
    fn the_pacer_holds_240_fps_to_a_few_percent() {
        let mut p = Pacer::new(240.0);
        let t = Instant::now();
        for _ in 0..120 {
            p.wait();
        }
        let secs = t.elapsed().as_secs_f64();
        assert!((0.48..0.70).contains(&secs), "120 frames took {secs} s");
    }
}
