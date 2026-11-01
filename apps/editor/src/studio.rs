//! Everything between the document and the 3D pane that needs no widgets:
//! the running build, the built level, the free-fly camera and Instant Play.
//! The xui shell calls [`Studio::tick`] on a timer and shows the frame it
//! returns, so the whole edit, build, play, return loop is testable headless.

use std::time::Duration;

use miniscent_game::FrameStats;
use mscent_platform::Event;
use mscent_render::{Framebuffer, Quality, draw_text};

use crate::build::{self, BuildJob, Built, STEPS, Update};
use crate::model::{Document, FlyCam};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Edit,
    Play,
}

/// What happened during one [`Studio::tick`].
#[derive(Debug, Default)]
pub struct Tick {
    /// The frame changed and should be shown.
    pub redraw: bool,
    /// Text for the status bar, when it changed.
    pub status: Option<String>,
    /// New log lines.
    pub log: Vec<String>,
    /// A build just finished successfully with `then_play` set.
    pub play_now: bool,
    /// Play ended by itself (the game asked to quit).
    pub left_play: bool,
}

pub struct Studio {
    job: Option<BuildJob>,
    built: Option<(Built, Document)>,
    pub mode: Mode,
    pub fly: FlyCam,
    /// Progress of the running build, `0..=STEPS`.
    pub progress: u32,
    fb: Option<Framebuffer>,
    redraw: bool,
}

/// The internal render size for a pane of `pane` pixels: the pane divided by
/// the smallest whole factor that brings the width to `max_width` or less.
pub fn frame_size(pane: (u32, u32), max_width: u32) -> (u32, u32) {
    let (w, h) = (pane.0.max(1), pane.1.max(1));
    let factor = w.div_ceil(max_width.max(1)).max(1);
    ((w / factor).max(16), (h / factor).max(16))
}

impl Studio {
    pub fn new(doc: &Document) -> Studio {
        Studio {
            job: None,
            built: None,
            mode: Mode::Edit,
            fly: FlyCam::for_document(doc),
            progress: 0,
            fb: None,
            redraw: true,
        }
    }

    pub fn building(&self) -> bool {
        self.job.is_some()
    }

    pub fn has_build(&self) -> bool {
        self.built.is_some()
    }

    /// Whether the built level no longer matches `doc`.
    pub fn stale(&self, doc: &Document) -> bool {
        self.built.as_ref().is_some_and(|(_, d)| d != doc)
    }

    /// Resets the camera for a new document and forgets the build.
    pub fn reset(&mut self, doc: &Document) {
        self.job = None;
        self.built = None;
        self.mode = Mode::Edit;
        self.fly = FlyCam::for_document(doc);
        self.redraw = true;
    }

    /// Starts a build unless one is running. `then_play` starts Play when it
    /// succeeds. Errors are messages for the status bar and log.
    pub fn start_build(&mut self, doc: &Document, then_play: bool) -> Result<(), String> {
        if self.job.is_some() {
            return Err("a build is already running".into());
        }
        self.job = Some(build::start(doc, then_play)?);
        self.progress = 0;
        Ok(())
    }

    /// Enters Play on the current build.
    pub fn enter_play(&mut self) -> Result<(), String> {
        let (built, _) = self.built.as_mut().ok_or("build the level first")?;
        let why = built.unplayable().map(str::to_string);
        let session = built.session_mut().ok_or_else(|| {
            format!(
                "cannot play: {}",
                why.unwrap_or_else(|| "not playable".into())
            )
        })?;
        session.restart();
        self.mode = Mode::Play;
        self.redraw = true;
        Ok(())
    }

    /// Back to editing; the document was never touched by Play.
    pub fn leave_play(&mut self) {
        self.mode = Mode::Edit;
        self.redraw = true;
    }

    /// Marks the picture as needing a redraw (e.g. the document went stale).
    pub fn invalidate(&mut self) {
        self.redraw = true;
    }

    /// Advances by `dt` with the pane's `events`: polls the build, flies the
    /// camera or steps the game.
    pub fn tick(&mut self, dt: Duration, events: &[Event]) -> Tick {
        let mut out = Tick::default();
        self.poll_build(&mut out);
        match self.mode {
            Mode::Edit => {
                let mut moved = false;
                for e in events {
                    moved |= self.fly.event(e);
                }
                moved |= self.fly.step(dt.as_secs_f32());
                if moved {
                    self.redraw = true;
                }
            }
            Mode::Play => {
                let quit = self
                    .built
                    .as_mut()
                    .and_then(|(b, _)| b.session_mut())
                    .is_some_and(|s| {
                        let secs = dt.as_secs_f32();
                        s.set_stats(FrameStats {
                            fps: if secs > 0.0 { 1.0 / secs } else { 0.0 },
                            frame_ms: secs * 1000.0,
                        });
                        s.step(dt, events).quit
                    });
                self.redraw = true;
                if quit {
                    self.leave_play();
                    out.left_play = true;
                    out.status = Some("Left the game; back in the editor".into());
                }
            }
        }
        out.redraw = std::mem::take(&mut self.redraw);
        out
    }

    fn poll_build(&mut self, out: &mut Tick) {
        let Some(job) = self.job.as_mut() else { return };
        let then_play = job.then_play;
        let started = job.started;
        let mut finished = None;
        for u in job.poll() {
            match u {
                Update::Step { step, text } => {
                    self.progress = step;
                    out.status = Some(format!("Building {step}/{STEPS}: {text}"));
                    out.log.push(text);
                }
                Update::Finished(r) => finished = Some(r),
            }
        }
        let Some(result) = finished else { return };
        let job = self.job.take();
        match result {
            Ok(built) => {
                let doc = job.map(|j| j.doc).unwrap_or_default();
                let note = built
                    .unplayable()
                    .map(|w| format!(" (view only: {w})"))
                    .unwrap_or_default();
                out.status = Some(format!(
                    "Build finished in {:.1} s{note}",
                    started.elapsed().as_secs_f32()
                ));
                out.play_now = then_play && built.unplayable().is_none();
                if then_play && !out.play_now {
                    out.status = Some(format!("Cannot play{note}"));
                }
                self.built = Some((built, doc));
            }
            Err(e) => {
                out.status = Some(format!("Build failed: {e}"));
                out.log.push(format!("ERROR: {e}"));
            }
        }
        self.progress = 0;
        self.redraw = true;
    }

    /// Renders the current picture at `w` x `h`: the game view in Play, the
    /// fly camera otherwise. `None` before any build or on a bad size.
    pub fn render(&mut self, w: u32, h: u32, doc: &Document) -> Option<&Framebuffer> {
        let stale = self.stale(doc);
        let view = self.fly.view();
        let play = self.mode == Mode::Play;
        let (built, _) = self.built.as_mut()?;
        if play {
            let session = built.session_mut()?;
            return session.render(w, h);
        }
        if self.fb.as_ref().map(|f| (f.width(), f.height())) != Some((w, h)) {
            self.fb = Some(Framebuffer::new(w, h)?);
        }
        let fb = self.fb.as_mut()?;
        let actors = built.renderer().actors().to_vec();
        built
            .renderer()
            .render(fb, &view, Quality::High, &actors, None)?;
        if stale {
            draw_text(fb, 6, 6, "OUT OF DATE: F7", 0xffff_c040, 1);
        }
        self.fb.as_ref()
    }
}

/// What the 3D pane's controls are, for its placeholder text.
pub const HELP: &str =
    "RMB drag or arrows: look   W/A/S/D: move   R/F: up/down   Q/E: roll   Shift: fast";

#[cfg(test)]
mod tests;
