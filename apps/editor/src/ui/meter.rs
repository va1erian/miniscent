//! A frame meter for `--live-timing`: while it is on, the 2D views' and the 3D
//! pane's painters, the UI tick and the gesture steps note when they ran and for
//! how long. It measures the real window on whichever backend draws it, which
//! the headless `--ui-timing` cannot: there, nothing presents and no timer runs.
//!
//! Off, each note is one thread-local read. Everything runs on the UI thread.

use std::cell::{Cell, RefCell};
use std::time::{Duration, Instant};

/// What a note is about.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Lane {
    /// One 2D view painted.
    View,
    /// The 3D pane painted.
    Pane,
    /// A new picture from the render thread reached the 3D pane.
    Picture,
    /// The UI timer's tick ran.
    Tick,
    /// One step of a scripted gesture: what a mouse move costs the model.
    Step,
}

thread_local! {
    static ON: Cell<bool> = const { Cell::new(false) };
    static NOTES: RefCell<Vec<(Lane, Instant, Duration)>> = const { RefCell::new(Vec::new()) };
}

/// Starts (or stops) recording.
pub fn set_on(on: bool) {
    ON.with(|o| o.set(on));
}

/// Whether notes are recorded, so a caller can skip reading the clock.
pub fn on() -> bool {
    ON.with(Cell::get)
}

/// Notes that `lane` ran from `start` until now.
pub fn note(lane: Lane, start: Instant) {
    if on() {
        NOTES.with(|n| n.borrow_mut().push((lane, start, start.elapsed())));
    }
}

/// Times `work` into `lane` when the meter is on.
pub fn time<R>(lane: Lane, work: impl FnOnce() -> R) -> R {
    if !on() {
        return work();
    }
    let start = Instant::now();
    let out = work();
    note(lane, start);
    out
}

/// Every note since the last take, oldest first.
pub fn take() -> Vec<(Lane, Instant, Duration)> {
    NOTES.with(|n| std::mem::take(&mut *n.borrow_mut()))
}

/// Views painted closer than this to the end of the previous one belong to the
/// same window paint: one `WM_PAINT` paints every dirty view back to back.
const SAME_FRAME: Duration = Duration::from_millis(2);

/// The frames of `lane`: (start, total paint time), with paints of one window
/// paint merged.
fn frames(notes: &[(Lane, Instant, Duration)], lane: Lane) -> Vec<(Instant, Duration)> {
    let mut out: Vec<(Instant, Duration)> = Vec::new();
    let mut last_end: Option<Instant> = None;
    for &(_, start, cost) in notes.iter().filter(|n| n.0 == lane) {
        match (out.last_mut(), last_end) {
            (Some(frame), Some(end)) if start.saturating_duration_since(end) < SAME_FRAME => {
                frame.1 += cost;
            }
            _ => out.push((start, cost)),
        }
        last_end = Some(start + cost);
    }
    out
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

/// The value at fraction `q` of `sorted`.
fn quantile(sorted: &[f64], q: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let i = ((sorted.len() - 1) as f64 * q).round() as usize;
    sorted[i.min(sorted.len() - 1)]
}

/// One line about `lane` over `span`: how often it ran, the gaps between runs
/// (what the eye sees as stutter) and what each run cost.
fn line(notes: &[(Lane, Instant, Duration)], lane: Lane, span: Duration) -> String {
    let runs = match lane {
        Lane::View | Lane::Pane => frames(notes, lane),
        Lane::Tick | Lane::Step | Lane::Picture => notes
            .iter()
            .filter(|n| n.0 == lane)
            .map(|n| (n.1, n.2))
            .collect(),
    };
    let mut gaps: Vec<f64> = runs.windows(2).map(|w| ms(w[1].0 - w[0].0)).collect();
    let mut costs: Vec<f64> = runs.iter().map(|r| ms(r.1)).collect();
    gaps.sort_by(f64::total_cmp);
    costs.sort_by(f64::total_cmp);
    let rate = runs.len() as f64 / span.as_secs_f64().max(1e-9);
    format!(
        "{:<7} {rate:6.1}/s  gap p50 {:6.2} p95 {:6.2} max {:6.2} ms  cost p50 {:6.2} p95 {:6.2} max {:6.2} ms",
        format!("{lane:?}").to_lowercase(),
        quantile(&gaps, 0.5),
        quantile(&gaps, 0.95),
        quantile(&gaps, 1.0),
        quantile(&costs, 0.5),
        quantile(&costs, 0.95),
        quantile(&costs, 1.0),
    )
}

/// The report of one phase: a line per lane that ran.
pub fn report(what: &str, notes: &[(Lane, Instant, Duration)], span: Duration) -> String {
    let mut out = format!("live {what} ({:.1} s)", span.as_secs_f64());
    for lane in [
        Lane::Step,
        Lane::Tick,
        Lane::View,
        Lane::Pane,
        Lane::Picture,
    ] {
        if notes.iter().any(|n| n.0 == lane) {
            out.push_str("\n    ");
            out.push_str(&line(notes, lane, span));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paints_of_one_window_paint_are_one_frame() {
        let t = Instant::now();
        let d = Duration::from_millis(1);
        let notes = [
            (Lane::View, t, d),
            (Lane::View, t + d, d),
            (Lane::View, t + Duration::from_millis(16), d),
            (Lane::Pane, t, d),
        ];
        let f = frames(&notes, Lane::View);
        assert_eq!(f.len(), 2);
        assert_eq!(f[0].1, 2 * d);
    }

    #[test]
    fn notes_only_while_on() {
        note(Lane::Tick, Instant::now());
        assert!(take().is_empty());
        set_on(true);
        time(Lane::Tick, || ());
        set_on(false);
        assert_eq!(take().len(), 1);
    }

    #[test]
    fn report_names_each_lane_that_ran() {
        let t = Instant::now();
        let notes = [(Lane::Tick, t, Duration::ZERO)];
        let r = report("idle", &notes, Duration::from_secs(1));
        assert!(r.contains("tick"));
        assert!(!r.contains("view"));
    }
}
