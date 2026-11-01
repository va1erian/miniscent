//! The main loop: poll the platform, step the session at 120 Hz inside it,
//! render at an adaptive internal resolution, present.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use mscent_platform::Platform;
use mscent_render::png_encode::encode_png;
use mscent_render::resolution::{ResolutionConfig, ResolutionController};

use crate::session::{FrameStats, GameSession};

/// Longest frame time the simulation is told about (a stall must not become
/// a burst of catch-up steps).
const MAX_FRAME: Duration = Duration::from_millis(100);

/// How a run is paced and observed.
#[derive(Clone, Debug)]
pub struct RunOptions {
    /// Internal resolution at full quality; the platform scales it up.
    pub base: (u32, u32),
    /// Lower the internal resolution to hold the frame budget.
    pub adaptive: bool,
    /// Sleep to this frame rate; `None` runs flat out.
    pub cap_fps: Option<f32>,
    /// Quit when the platform clock passes this.
    pub autoclose: Option<Duration>,
    /// Platform-clock seconds at which to write a PNG of the frame.
    pub shot_times: Vec<f64>,
    /// `<stem>_<t>s.png` is written for each shot time.
    pub shot_stem: Option<PathBuf>,
}

/// What a run measured. Times are real (wall clock) milliseconds of the
/// render call, which is what the frame budget is about.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RunReport {
    pub frames: u64,
    pub avg_ms: f64,
    pub p95_ms: f64,
    pub max_ms: f64,
    /// Internal resolution scale at the end, in per-mille.
    pub final_scale: u32,
    pub shots: Vec<PathBuf>,
}

fn percentile(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let i = ((sorted.len() - 1) as f64 * p).round() as usize;
    sorted[i.min(sorted.len() - 1)]
}

fn shot_path(stem: &std::path::Path, t: f64) -> PathBuf {
    let name = stem
        .file_stem()
        .map_or_else(|| "shot".to_string(), |s| s.to_string_lossy().into_owned());
    stem.with_file_name(format!("{name}_{t}s.png"))
}

/// Runs until the platform asks to quit, the player quits or the autoclose
/// time passes.
pub fn run(platform: &mut dyn Platform, session: &mut GameSession, opts: &RunOptions) -> RunReport {
    let mut controller = ResolutionController::new(ResolutionConfig::default());
    let mut costs: Vec<f64> = Vec::new();
    let mut shots = Vec::new();
    let mut next_shot = 0;
    let mut captured = false;
    let mut last = platform.now();
    let mut smooth_ms = 16.6f32;
    let target = opts
        .cap_fps
        .map(|f| Duration::from_secs_f32(1.0 / f.max(1.0)));
    while !platform.should_quit() {
        let frame_start = Instant::now();
        let events = platform.poll_events();
        let now = platform.now();
        let dt = now.saturating_sub(last).min(MAX_FRAME);
        last = now;
        let outcome = session.step(dt, &events);
        if outcome.quit || opts.autoclose.is_some_and(|d| now >= d) {
            break;
        }
        if outcome.capture_mouse != captured {
            captured = outcome.capture_mouse;
            platform.set_mouse_captured(captured);
        }
        let (w, h) = match (&controller, opts.adaptive) {
            (Some(c), true) => c.internal_size(opts.base.0, opts.base.1),
            _ => opts.base,
        };
        let render_start = Instant::now();
        let Some(fb) = session.render(w, h) else {
            break;
        };
        let cost = render_start.elapsed();
        platform.present(fb.pixels(), w, h);
        if let (Some(stem), Some(&t)) = (&opts.shot_stem, opts.shot_times.get(next_shot))
            && now.as_secs_f64() >= t
        {
            next_shot += 1;
            if let Some(png) = encode_png(w, h, fb.pixels()) {
                let path = shot_path(stem, t);
                if std::fs::write(&path, png).is_ok() {
                    shots.push(path);
                }
            }
        }
        let ms = cost.as_secs_f64() * 1000.0;
        costs.push(ms);
        if let Some(c) = controller.as_mut() {
            c.update(u32::try_from(cost.as_micros()).unwrap_or(u32::MAX));
        }
        if let Some(target) = target {
            pace(frame_start, target);
        }
        let total_ms = frame_start.elapsed().as_secs_f32() * 1000.0;
        smooth_ms += (total_ms - smooth_ms) * 0.1;
        session.set_stats(FrameStats {
            fps: 1000.0 / smooth_ms.max(0.01),
            frame_ms: ms as f32,
        });
    }
    summarize(
        costs,
        controller.map_or(1000, |c| c.scale_permille()),
        shots,
    )
}

/// Sleeps most of the remaining frame time, then spins the last bit, since
/// the OS sleep is coarse.
fn pace(start: Instant, target: Duration) {
    let spin = Duration::from_millis(2);
    if let Some(left) = target.checked_sub(start.elapsed())
        && left > spin
    {
        std::thread::sleep(left - spin);
    }
    while start.elapsed() < target {
        std::hint::spin_loop();
    }
}

fn summarize(mut costs: Vec<f64>, final_scale: u32, shots: Vec<PathBuf>) -> RunReport {
    let frames = costs.len() as u64;
    let avg_ms = costs.iter().sum::<f64>() / costs.len().max(1) as f64;
    costs.sort_by(f64::total_cmp);
    RunReport {
        frames,
        avg_ms,
        p95_ms: percentile(&costs, 0.95),
        max_ms: costs.last().copied().unwrap_or(0.0),
        final_scale,
        shots,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percentile_and_summary_handle_empty_and_small_runs() {
        assert_eq!(percentile(&[], 0.95), 0.0);
        let r = summarize(vec![3.0, 1.0, 2.0], 750, vec![]);
        assert_eq!((r.frames, r.max_ms, r.final_scale), (3, 3.0, 750));
        assert!((r.avg_ms - 2.0).abs() < 1e-9);
        assert_eq!(summarize(vec![], 1000, vec![]).frames, 0);
    }

    #[test]
    fn shot_paths_append_the_time_to_the_stem() {
        let p = shot_path(std::path::Path::new("out/play.png"), 1.5);
        assert_eq!(p, PathBuf::from("out/play_1.5s.png"));
    }
}
