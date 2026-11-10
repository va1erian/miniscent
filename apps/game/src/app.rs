//! The main loop: poll the platform, step the session at 120 Hz inside it,
//! render at an adaptive internal resolution, present.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use mscent_platform::Platform;
use mscent_render::camera::MAX_SIDE;
use mscent_render::png_encode::encode_png;
use mscent_render::resolution::{ResolutionConfig, ResolutionController};
use mscent_render::{FrameInput, Framebuffer, LevelAssets};

use crate::scripts::ScriptWatcher;
use crate::session::{FrameStats, GameSession};

/// How often scripts are checked for changes, in platform seconds.
const WATCH_SECONDS: f64 = 0.25;

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
    /// Reload the level's scripts when they change on disk.
    pub watch: Option<ScriptWatcher>,
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
    /// Why the run stopped before it was done: a renderer asked for that could
    /// not draw.
    pub stopped: Option<String>,
}

/// What draws a frame's world where software does not: the frame is read back
/// into the framebuffer software would fill, so what goes over it is drawn as
/// before.
pub trait World {
    /// Draws `frame` of `assets` into `fb`. False leaves it to software.
    fn draw(&mut self, assets: &LevelAssets, fb: &mut Framebuffer, frame: &FrameInput) -> bool;

    /// Whether this world shows its frames in the window itself, through
    /// [`World::show`], rather than giving them to the platform to present.
    fn shows(&self) -> bool {
        false
    }

    /// Shows `frame` of `assets` in the window with what `over` says over it.
    /// False leaves the frame to software and the platform.
    fn show(&mut self, _: &LevelAssets, _: &FrameInput, _: &Shown) -> bool {
        false
    }

    /// Why the run must stop, for a world that may not leave frames to software.
    fn stop(&self) -> Option<String> {
        None
    }
}

/// What goes over a frame [`World::show`] shows, and where.
pub struct Shown<'a> {
    /// What the HUD, feedback, menus and notices drew, from cleared to nothing.
    pub layer: &'a Framebuffer,
    /// The white flash between the world and the layer, `0..=256`.
    pub white: u32,
    /// The window's inner size, and the world's, which is drawn to fill it.
    pub window: (u32, u32),
    pub world: (u32, u32),
}

/// What a world that shows itself draws in a window of `window` pixels: the world
/// at the window's own size, or what `scaled` makes of it, and the layer at about
/// `base`'s height, the window's shape at a whole fraction of its size, so that
/// the HUD's pixels read as they do at the base size. A window with no area, as
/// a minimised one, draws at `base`, and shows nothing.
pub fn shown_sizes(
    window: (u32, u32),
    base: (u32, u32),
    scaled: impl Fn((u32, u32)) -> (u32, u32),
) -> ((u32, u32), (u32, u32)) {
    if window.0 == 0 || window.1 == 0 {
        return (base, base);
    }
    let window = (window.0.min(MAX_SIDE), window.1.min(MAX_SIDE));
    let k = (window.1 + base.1 / 2) / base.1.max(1);
    let k = k.max(1);
    let layer = ((window.0 / k).max(16), (window.1 / k).max(16));
    let (w, h) = scaled(window);
    ((w.max(16), h.max(16)), layer)
}

/// Software draws every world.
pub struct Software;

impl World for Software {
    fn draw(&mut self, _: &LevelAssets, _: &mut Framebuffer, _: &FrameInput) -> bool {
        false
    }
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
/// time passes, with software drawing the world.
pub fn run(platform: &mut dyn Platform, session: &mut GameSession, opts: &RunOptions) -> RunReport {
    run_with(platform, session, opts, &mut Software)
}

/// [`run`], with `world` drawing the world where it can.
pub fn run_with(
    platform: &mut dyn Platform,
    session: &mut GameSession,
    opts: &RunOptions,
    world: &mut dyn World,
) -> RunReport {
    // The resolution budget is one frame of the cap, so a 120 fps run lowers
    // the resolution at 8 ms, not at 16.
    let mut config = ResolutionConfig::default();
    if let Some(fps) = opts.cap_fps {
        config.budget_us = (1_000_000.0 / fps.max(1.0)) as u32;
    }
    let mut controller = ResolutionController::new(config);
    let mut costs: Vec<f64> = Vec::new();
    let mut shots = Vec::new();
    let mut next_shot = 0;
    let mut captured = false;
    let mut last = platform.now();
    let mut smooth_ms = 16.6f32;
    let mut watch = opts.watch.clone();
    let mut next_watch = 0.0;
    let mut pacer = opts.cap_fps.map(Pacer::new);
    while !platform.should_quit() {
        let frame_start = Instant::now();
        let events = platform.poll_events();
        let now = platform.now();
        let dt = now.saturating_sub(last).min(MAX_FRAME);
        last = now;
        if let Some(watcher) = watch.as_mut()
            && now.as_secs_f64() >= next_watch
        {
            next_watch = now.as_secs_f64() + WATCH_SECONDS;
            match watcher.reload_if_changed(session) {
                Ok(true) => eprintln!("script reloaded"),
                Ok(false) => {}
                Err(problem) => eprintln!("{problem}\nthe old script keeps running"),
            }
        }
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
        let fb = if world.shows() {
            let window = platform.size();
            let sizes = shown_sizes(window, opts.base, |(ww, wh)| {
                match (&controller, opts.adaptive) {
                    (Some(c), true) => c.internal_size(ww, wh),
                    _ => (ww, wh),
                }
            });
            let shown = session.render_layered(sizes, &mut |assets, frame, layer, white| {
                let world_size = sizes.0;
                world.show(
                    assets,
                    frame,
                    &Shown {
                        layer,
                        white,
                        window,
                        world: world_size,
                    },
                )
            });
            let Some(fb) = shown else {
                break;
            };
            fb
        } else {
            let drawn = session
                .render_through(w, h, &mut |assets, fb, frame| world.draw(assets, fb, frame));
            let Some(fb) = drawn else {
                break;
            };
            Some(fb)
        };
        if world.stop().is_some() {
            break;
        }
        let cost = render_start.elapsed();
        if let Some(fb) = fb {
            platform.present(fb.pixels(), fb.width(), fb.height());
        }
        // A frame the world showed itself is not on the CPU to be written.
        if let (Some(stem), Some(&t), Some(fb)) =
            (&opts.shot_stem, opts.shot_times.get(next_shot), fb)
            && now.as_secs_f64() >= t
        {
            next_shot += 1;
            if let Some(png) = encode_png(fb.width(), fb.height(), fb.pixels()) {
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
        if let Some(pacer) = pacer.as_mut() {
            pacer.wait();
        }
        let total_ms = frame_start.elapsed().as_secs_f32() * 1000.0;
        smooth_ms += (total_ms - smooth_ms) * 0.1;
        // Wall-clock timings in the HUD would make unpaced (scripted,
        // headless) runs differ pixel for pixel, so only paced runs show them.
        if pacer.is_some() {
            session.set_stats(FrameStats {
                fps: 1000.0 / smooth_ms.max(0.01),
                frame_ms: ms as f32,
            });
        }
    }
    if captured {
        platform.set_mouse_captured(false);
    }
    let report = summarize(
        costs,
        controller.map_or(1000, |c| c.scale_permille()),
        shots,
    );
    RunReport {
        stopped: world.stop(),
        ..report
    }
}

/// Holds frames to a steady rate. Each frame is due one period after the
/// last one was *due*, not after it ended, so an early finish or a late wake
/// does not shift the rhythm; only a stall longer than a few periods resets it
/// (the schedule must not rush to catch up).
struct Pacer {
    period: Duration,
    due: Instant,
}

impl Pacer {
    /// Longest the schedule may fall behind before it starts over.
    const MAX_LAG_PERIODS: u32 = 3;
    /// Slept short of the deadline, since the OS sleep is coarse; the rest spins.
    const SPIN: Duration = Duration::from_millis(2);

    fn new(fps: f32) -> Self {
        let period = Duration::from_secs_f32(1.0 / fps.clamp(1.0, 1000.0));
        Self {
            period,
            due: Instant::now() + period,
        }
    }

    /// The deadline after the one due at `due`, given that it is `now`.
    fn next_due(&self, now: Instant) -> Instant {
        let next = self.due + self.period;
        if now.saturating_duration_since(next) > self.period * Self::MAX_LAG_PERIODS {
            now + self.period
        } else {
            next
        }
    }

    /// Blocks until the current frame's deadline, then schedules the next.
    fn wait(&mut self) {
        if let Some(left) = self.due.checked_duration_since(Instant::now())
            && left > Self::SPIN
        {
            std::thread::sleep(left - Self::SPIN);
        }
        while Instant::now() < self.due {
            std::hint::spin_loop();
        }
        self.due = self.next_due(Instant::now());
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
        stopped: None,
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
    fn a_window_draws_its_world_at_its_size_and_the_hud_at_the_base_height() {
        let same = |size| size;
        // A maximised window, not 16:9: the world fills it and the HUD keeps
        // the base's pixel size, in the window's shape.
        assert_eq!(
            shown_sizes((2000, 1088), (640, 360), same),
            ((2000, 1088), (666, 362))
        );
        assert_eq!(
            shown_sizes((3840, 2160), (640, 360), same),
            ((3840, 2160), (640, 360))
        );
        assert_eq!(
            shown_sizes((800, 400), (640, 360), same),
            ((800, 400), (800, 400))
        );
        let half = |(w, h): (u32, u32)| (w / 2, h / 2);
        assert_eq!(shown_sizes((3840, 2160), (640, 360), half).0, (1920, 1080));
        // Minimised: nothing to show, so the base, which a framebuffer can hold.
        assert_eq!(
            shown_sizes((0, 0), (640, 360), same),
            ((640, 360), (640, 360))
        );
        let huge = shown_sizes((20_000, 9_000), (640, 360), same);
        assert!(huge.0.0 <= MAX_SIDE && huge.0.1 <= MAX_SIDE, "{huge:?}");
    }

    #[test]
    fn the_pacer_keeps_its_rhythm_through_jitter_and_resets_after_a_stall() {
        let mut p = Pacer::new(240.0);
        let start = p.due;
        // A frame that woke 1 ms late does not push the next deadline back.
        assert_eq!(
            p.next_due(start + Duration::from_millis(1)),
            start + p.period
        );
        // A long stall starts the schedule over instead of bursting to catch up.
        let late = start + Duration::from_millis(500);
        assert_eq!(p.next_due(late), late + p.period);
        p.due = p.next_due(late);
        assert!(p.due > late);
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

    #[test]
    fn shot_paths_append_the_time_to_the_stem() {
        let p = shot_path(std::path::Path::new("out/play.png"), 1.5);
        assert_eq!(p, PathBuf::from("out/play_1.5s.png"));
    }
}
