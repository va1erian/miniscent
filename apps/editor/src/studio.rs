//! Everything between the document and the 3D pane that needs no widgets:
//! the running build, the built level, the free-fly camera and Instant Play.
//! The xui shell calls [`Studio::tick`] on a timer. Stepping the game and
//! drawing the 3D view run on a render thread (`worker.rs`), so a slow frame
//! never slows the UI; `tick` forwards input and returns the newest finished
//! frame without ever waiting. The whole edit, build, play, return loop is
//! testable headless.

use std::path::Path;
use std::sync::PoisonError;
use std::thread::JoinHandle;

use miniscent_game::lightcache;
use mscent_platform::Event;

use crate::build::{self, BuildJob, STEPS, SharedCache, Update};
use crate::model::{Document, FlyCam};

mod highlight;
mod scene;
pub mod worker;

use highlight::Highlights;
use scene::LevelScene;
pub use worker::{Frame, Scene};
use worker::{Slow, Worker};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Edit,
    Play,
}

/// What happened during one [`Studio::tick`].
#[derive(Default)]
pub struct Tick {
    /// The newest finished picture, when there is one to show.
    pub frame: Option<Frame>,
    /// Text for the status bar, when it changed.
    pub status: Option<String>,
    /// New log lines.
    pub log: Vec<String>,
    /// A build just finished successfully with `then_play` set.
    pub play_now: bool,
    /// Play ended by itself (the game asked to quit, or the thread died).
    pub left_play: bool,
}

/// The build, the render thread and the mode.
pub struct Studio {
    job: Option<BuildJob>,
    /// The document the current build was made from.
    built_doc: Option<Document>,
    unplayable: Option<String>,
    worker: Option<Worker>,
    handle: Option<JoinHandle<()>>,
    /// Threads told to stop, dropped once they have finished.
    retired: Vec<JoinHandle<()>>,
    pub mode: Mode,
    camera: FlyCam,
    /// Progress of the running build, `0..=STEPS`.
    pub progress: u32,
    check_stale: bool,
    /// The last lightmap, so a rebuild after a pure texture edit skips the bake.
    cache: SharedCache,
    highlights: Highlights,
    /// The document changed since texture-only edits were last looked for.
    recheck: bool,
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
            built_doc: None,
            unplayable: None,
            worker: None,
            handle: None,
            retired: Vec::new(),
            mode: Mode::Edit,
            camera: FlyCam::for_document(doc),
            progress: 0,
            check_stale: false,
            cache: SharedCache::default(),
            highlights: Highlights::default(),
            recheck: false,
        }
    }

    pub fn building(&self) -> bool {
        self.job.is_some()
    }

    pub fn has_build(&self) -> bool {
        self.built_doc.is_some()
    }

    /// Whether the built level no longer matches `doc`.
    pub fn stale(&self, doc: &Document) -> bool {
        self.built_doc.as_ref().is_some_and(|d| d != doc)
    }

    /// Tells the picture whether it is out of date.
    pub fn set_stale(&self, stale: bool) {
        if let Some(w) = &self.worker {
            w.stale(stale);
        }
    }

    /// Render threads still running, including ones told to stop.
    #[cfg(test)]
    pub fn worker_threads(&self) -> usize {
        let retired = self.retired.iter().filter(|h| !h.is_finished()).count();
        usize::from(self.handle.is_some()) + retired
    }

    /// Resets the camera for a new document and forgets the build.
    pub fn reset(&mut self, doc: &Document) {
        self.job = None;
        self.retire_worker();
        self.built_doc = None;
        self.unplayable = None;
        self.mode = Mode::Edit;
        self.camera = FlyCam::for_document(doc);
    }

    /// Puts the fly camera back where a new document starts.
    pub fn reset_camera(&mut self, doc: &Document) {
        self.set_camera(FlyCam::for_document(doc));
    }

    /// The fly camera as it is right now.
    pub fn fly_camera(&self) -> FlyCam {
        self.worker
            .as_ref()
            .and_then(Worker::current_camera)
            .unwrap_or_else(|| self.camera.clone())
    }

    /// Sets the faces to outline in the 3D view. Nothing is sent when they
    /// are the ones already shown.
    pub fn set_highlights(&mut self, polys: Vec<Vec<mscent_math::Vec3>>) {
        if *self.highlights == polys {
            return;
        }
        self.highlights = Highlights::new(polys);
        if let Some(w) = &self.worker {
            w.highlights(&self.highlights);
        }
    }

    /// The document changed: look for a texture-only edit on the next tick.
    pub fn note_edit(&mut self) {
        self.recheck = true;
    }

    /// The lightmap cache shared by every build.
    #[cfg(test)]
    pub fn cache(&self) -> SharedCache {
        self.cache.clone()
    }

    /// Points the lightmap cache at the file for `level`: `<level>.msl.lit`
    /// beside a saved level, else the per-user cache directory.
    pub fn set_cache_level(&mut self, level: Option<&Path>) {
        let path = lightcache::path_for(level, "untitled");
        self.cache
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .set_path(path);
    }

    /// Save: moves the cache file next to `level` and, when the build still
    /// has the geometry of `doc`, writes the current bake there. Returns a
    /// note for the status bar when a file was written.
    pub fn save_lightmap(&mut self, level: &Path, doc: &Document) -> Option<String> {
        let path = lightcache::path_for(Some(level), "untitled")?;
        let current = self
            .built_doc
            .as_ref()
            .is_some_and(|d| d.same_geometry(doc));
        let mut cache = self.cache.lock().unwrap_or_else(PoisonError::into_inner);
        if !current {
            cache.set_path(Some(path));
            return None;
        }
        match cache.save_to(path.clone()) {
            Ok(true) => Some(format!("lightmap cached in {}", path.display())),
            Ok(false) => None,
            Err(e) => Some(format!("lightmap cache not written: {e}")),
        }
    }

    /// Whether `doc` differs from the build only in materials and mappings,
    /// which the lightmap does not depend on.
    fn texture_only_change(&self, doc: &Document) -> bool {
        self.built_doc
            .as_ref()
            .is_some_and(|d| d != doc && d.same_geometry(doc))
    }

    /// After a texture-only edit: rebuilds in the background. The cached
    /// lightmap is reused, so the new mapping shows within a fraction of a
    /// second and the log stays quiet.
    fn refresh_textures(&mut self, doc: &Document) {
        if let Ok(mut job) = build::start_cached(doc, false, &self.cache) {
            job.quiet = true;
            self.job = Some(job);
        }
    }

    /// Moves the fly camera.
    pub fn set_camera(&mut self, fly: FlyCam) {
        if let Some(w) = &self.worker {
            w.camera(&fly);
        }
        self.camera = fly;
    }

    /// Starts a build unless one is running. `then_play` starts Play when it
    /// succeeds. Errors are messages for the status bar and log.
    pub fn start_build(&mut self, doc: &Document, then_play: bool) -> Result<(), String> {
        if self.job.is_some() {
            return Err("a build is already running".into());
        }
        self.job = Some(build::start_cached(doc, then_play, &self.cache)?);
        self.progress = 0;
        Ok(())
    }

    /// Enters Play on the current build.
    pub fn enter_play(&mut self) -> Result<(), String> {
        let Some(w) = &self.worker else {
            return Err("build the level first".into());
        };
        if let Some(why) = &self.unplayable {
            return Err(format!("cannot play: {why}"));
        }
        w.mode(Mode::Play);
        self.mode = Mode::Play;
        Ok(())
    }

    /// Back to editing; the document was never touched by Play.
    pub fn leave_play(&mut self) {
        self.mode = Mode::Edit;
        if let Some(w) = &self.worker {
            w.mode(Mode::Edit);
        }
    }

    /// Gives a new render thread a finished level (a build's product),
    /// keeping the camera where the old one left it.
    fn attach(&mut self, scene: Box<dyn Scene>, doc: Document, unplayable: Option<String>) {
        if let Some(c) = self.worker.as_ref().and_then(Worker::current_camera) {
            self.camera = c;
        }
        self.retire_worker();
        let delay = worker::slow_ms();
        let scene: Box<dyn Scene> = if delay > 0 {
            Box::new(Slow {
                inner: scene,
                delay: std::time::Duration::from_millis(delay),
            })
        } else {
            scene
        };
        match Worker::spawn(scene, self.camera.clone()) {
            Ok((w, h)) => {
                w.highlights(&self.highlights);
                self.worker = Some(w);
                self.handle = Some(h);
                self.built_doc = Some(doc);
                self.unplayable = unplayable;
                self.check_stale = true;
            }
            Err(_) => self.built_doc = None,
        }
    }

    /// Starts a render thread on `scene` as if a build of `doc` had finished
    /// (tests drive the thread with their own scenes).
    #[cfg(test)]
    pub fn attach_scene(&mut self, scene: Box<dyn Scene>, doc: &Document) {
        self.attach(scene, doc.clone(), None);
    }

    fn retire_worker(&mut self) {
        if let Some(w) = self.worker.take() {
            w.stop();
        }
        if let Some(h) = self.handle.take() {
            self.retired.push(h);
        }
    }

    /// Forwards the pane's `events`, polls the build and collects what the
    /// render thread produced. `pane` is the visible pane's size in pixels
    /// (zero when hidden); `doc` the current document. Never blocks.
    pub fn tick(&mut self, events: &[Event], pane: (u32, u32), doc: &Document) -> Tick {
        let mut out = Tick::default();
        self.poll_build(&mut out);
        self.retired.retain(|h| !h.is_finished());
        if self.recheck && self.job.is_none() && self.mode == Mode::Edit && self.worker.is_some() {
            self.recheck = false;
            if self.texture_only_change(doc) {
                self.refresh_textures(doc);
            }
        }
        if std::mem::take(&mut self.check_stale) {
            self.set_stale(self.stale(doc));
        }
        let size = if pane.0 == 0 || pane.1 == 0 {
            (0, 0)
        } else {
            frame_size(pane, 960)
        };
        let Some(w) = self.worker.as_mut() else {
            return out;
        };
        w.size(size.0, size.1);
        w.input(events);
        let error = w.take_error();
        if error.is_some() || !w.alive() {
            let why = error.unwrap_or_else(|| "the render thread stopped".into());
            self.lose_worker(&why, &mut out);
            return out;
        }
        if self.mode == Mode::Play && w.take_quit() {
            out.left_play = true;
            out.status = Some("Left the game; back in the editor".into());
        }
        out.frame = w.take_frame().filter(|f| f.mode == self.mode);
        if out.left_play {
            self.leave_play();
        }
        out
    }

    /// The render thread died: report it and fall back to editing. The
    /// document is untouched; building again starts a fresh thread.
    fn lose_worker(&mut self, why: &str, out: &mut Tick) {
        self.retire_worker();
        self.built_doc = None;
        out.left_play = self.mode == Mode::Play;
        self.mode = Mode::Edit;
        out.status = Some(format!("Render thread failed: {why}"));
        out.log.push(format!("ERROR: render thread failed: {why}"));
    }

    fn poll_build(&mut self, out: &mut Tick) {
        let Some(job) = self.job.as_mut() else { return };
        let then_play = job.then_play;
        let quiet = job.quiet;
        let started = job.started;
        let mut finished = None;
        for u in job.poll() {
            match u {
                Update::Step { step, text } => {
                    self.progress = step;
                    if !quiet {
                        out.status = Some(format!("Building {step}/{STEPS}: {text}"));
                        out.log.push(text);
                    }
                }
                Update::Finished(r) => finished = Some(r),
            }
        }
        let Some(result) = finished else { return };
        let job = self.job.take();
        match result {
            Ok(built) => {
                let doc = job.map(|j| j.doc).unwrap_or_default();
                let why = built.unplayable().map(str::to_string);
                let note = why
                    .as_ref()
                    .map(|w| format!(" (view only: {w})"))
                    .unwrap_or_default();
                let lit = built.report.as_ref().map_or("", |r| match r.source {
                    mscent_render::Source::Baked => "; lightmap baked",
                    mscent_render::Source::File => "; lightmap loaded from cache",
                    mscent_render::Source::Memory => "; lightmap reused, no bake",
                });
                out.status = Some(format!(
                    "Build finished in {:.1} s{lit}{note}",
                    started.elapsed().as_secs_f32()
                ));
                if quiet {
                    let how = match built.bake {
                        Some(mscent_render::Source::Baked) => "re-baked",
                        _ => "lightmap kept",
                    };
                    out.status = Some(format!(
                        "Texture mapping updated in {} ms ({how})",
                        started.elapsed().as_millis()
                    ));
                }
                out.play_now = then_play && why.is_none();
                if then_play && !out.play_now {
                    out.status = Some(format!("Cannot play{note}"));
                }
                self.mode = Mode::Edit;
                self.attach(Box::new(LevelScene::new(built)), doc, why);
            }
            Err(e) => {
                out.status = Some(format!("Build failed: {e}"));
                out.log.push(format!("ERROR: {e}"));
            }
        }
        self.progress = 0;
    }
}

impl Drop for Studio {
    /// Stops the render thread and waits for it (only at exit, never while
    /// the UI is live).
    fn drop(&mut self) {
        self.retire_worker();
        for h in self.retired.drain(..) {
            let _ = h.join();
        }
    }
}

/// What the 3D pane's controls are, for its placeholder text.
pub const HELP: &str = "RMB drag or arrows: look
W/A/S/D: move   R/F: up/down
Q/E: roll   Shift: fast";

#[cfg(test)]
mod race_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod texture_tests;
