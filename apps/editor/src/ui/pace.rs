//! A message on a steady beat from a thread of its own.
//!
//! The UI's frame tick came from `Ui::every(16)`, a Win32 `SetTimer`, which
//! Windows rounds up to its 15.6 ms clock: a 16 ms timer fires every 31 ms, so
//! the 3D pane, which shows a new picture only on a tick, ran at 32 frames a
//! second however fast the render thread was. A thread sleeping to a deadline
//! keeps 60 (std's sleep uses a high-resolution timer on Windows).
//!
//! A beat is sent only once the last one was handled, so a UI thread that falls
//! behind skips beats rather than working through a backlog of them.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use xui::app::Proxy;

/// One frame at 60 Hz.
pub const FRAME: Duration = Duration::from_nanos(16_666_667);

/// The UI side of a beat: says when one was handled.
pub struct Pacer {
    due: Arc<AtomicBool>,
}

impl Pacer {
    /// Sends `msg` every `period` while the window lives.
    pub fn start<M: Send + Clone + 'static>(proxy: Proxy<M>, msg: M, period: Duration) -> Pacer {
        let due = Arc::new(AtomicBool::new(false));
        let flag = due.clone();
        std::thread::spawn(move || {
            let mut next = Instant::now();
            loop {
                if !flag.swap(true, Ordering::AcqRel) && proxy.send(msg.clone()).is_err() {
                    return;
                }
                next = following(next, period, Instant::now());
                std::thread::sleep(next.saturating_duration_since(Instant::now()));
            }
        });
        Pacer { due }
    }

    /// The beat was handled: the next one may be sent.
    pub fn arrived(&self) {
        self.due.store(false, Ordering::Release);
    }
}

/// The deadline after `last`: one period on, or the next one after `now` when
/// beats were missed, so a stall is not followed by a burst.
fn following(last: Instant, period: Duration, now: Instant) -> Instant {
    let next = last + period;
    if next >= now {
        return next;
    }
    let behind = (now - last).as_nanos() / period.as_nanos().max(1);
    last + period * u32::try_from(behind + 1).unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_beat_on_time_is_one_period_on() {
        let t = Instant::now();
        assert_eq!(following(t, FRAME, t), t + FRAME);
    }

    #[test]
    fn missed_beats_are_skipped_not_sent_in_a_burst() {
        let t = Instant::now();
        let late = t + FRAME * 3 + Duration::from_millis(1);
        let next = following(t, FRAME, late);
        assert!(next > late);
        assert!(next - late <= FRAME);
    }
}
