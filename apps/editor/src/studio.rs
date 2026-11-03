//! Everything between the document and the 3D pane that needs no widgets:
//! the jobs that keep the picture current, the built level, the free-fly
//! camera and Instant Play. The xui shell calls [`Studio::tick`] on a timer and
//! `preview.rs` starts whatever the document needs. Stepping the game and
//! drawing the 3D view run on a render thread (`worker.rs`), so a slow frame
//! never slows the UI; `tick` forwards input and returns the newest finished
//! frame without ever waiting. The whole edit, play, return loop is testable
//! headless.

use std::path::{Path, PathBuf};
use std::sync::TryLockError;
use std::thread::JoinHandle;
use std::time::Instant;

use miniscent_game::{SessionConfig, lightcache};
use mscent_platform::Event;

use crate::build::{BakeJob, Built, Job, Light, SharedCache};
use crate::materials::SharedMaterials;
use crate::model::{Document, FlyCam};

pub mod gpu_view;
pub use gpu_view::frame_size;
mod highlight;
mod live;
mod preview;
mod scene;
pub mod worker;

use highlight::Highlights;
pub use preview::Preview;
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
    /// The preview that a pending Play was waiting for is up.
    pub play_now: bool,
    /// The picture must go: nothing can be drawn. The text says why.
    pub blank: Option<String>,
    /// Play ended by itself (the game asked to quit, or the thread died).
    pub left_play: bool,
}

/// The preview and bake jobs, the render thread and the mode.
pub struct Studio {
    preview_job: Option<Job<Built>>,
    bake_job: Option<BakeJob>,
    /// The document last sent to a preview job, good or not: the same one is
    /// not tried twice.
    wanted: Option<Document>,
    /// When the document was last seen to change.
    changed_at: Instant,
    /// A bake finished or the materials changed: the next preview picks that up.
    refresh: bool,
    script_watch: preview::ScriptWatch,
    /// The document the picture was made from.
    built_doc: Option<Document>,
    /// The lighting of that picture.
    light: Light,
    /// Identifies its full lightmap.
    light_key: u64,
    /// The full lightmap that could not be baked.
    failed_bake: Option<u64>,
    /// Why the current document cannot be shown.
    error: Option<String>,
    /// Play starts when a preview of the current document is up.
    play_pending: bool,
    /// The empty level's text is up.
    empty: bool,
    /// The note last sent to the picture.
    banner: Option<&'static str>,
    unplayable: Option<String>,
    worker: Option<Worker>,
    handle: Option<JoinHandle<()>>,
    /// Threads told to stop, dropped once they have finished.
    retired: Vec<JoinHandle<()>>,
    pub mode: Mode,
    camera: FlyCam,
    /// The last lightmap, so an edit that keeps the faces and lights skips the
    /// bake.
    cache: SharedCache,
    /// A cache file path set while a job held the cache (`Some(None)`: no
    /// file), applied by the next tick.
    pending_path: Option<Option<PathBuf>>,
    /// The material table every build draws with.
    materials: SharedMaterials,
    /// The newest generation of it that a preview was asked for.
    materials_seen: u64,
    /// The generation of it in the picture.
    built_materials: u64,
    highlights: Highlights,
    /// How previews and bakes light the level.
    settings: SessionConfig,
    /// The voxel batch being meshed.
    live_job: Option<live::LiveJob>,
    /// The BSP of the picture, which live batches are meshed against.
    live_bsp: Option<std::sync::Arc<mscent_world::Bsp>>,
    /// A stroke is running.
    stroking: bool,
    /// The picture has surfaces the game session built with it does not: it
    /// collides with what the last preview compiled.
    live_dirty: bool,
    /// Batches meshed so far.
    live_count: usize,
    /// The vista warnings last put in the log, so an edit that leaves them as
    /// they were does not repeat them.
    warned: Vec<String>,
}

impl Studio {
    pub fn new(doc: &Document) -> Studio {
        Studio {
            preview_job: None,
            live_job: None,
            live_bsp: None,
            stroking: false,
            live_dirty: false,
            live_count: 0,
            warned: Vec::new(),
            bake_job: None,
            wanted: None,
            changed_at: Instant::now(),
            refresh: false,
            script_watch: Default::default(),
            built_doc: None,
            light: Light::Draft,
            light_key: 0,
            failed_bake: None,
            error: None,
            play_pending: false,
            empty: false,
            banner: None,
            unplayable: None,
            worker: None,
            handle: None,
            retired: Vec::new(),
            mode: Mode::Edit,
            camera: FlyCam::for_document(doc),
            cache: SharedCache::default(),
            pending_path: None,
            materials: SharedMaterials::default(),
            materials_seen: 0,
            built_materials: 0,
            highlights: Highlights::default(),
            settings: SessionConfig::default(),
        }
    }

    /// Whether the picture was made from `doc`, so Play can start on it now.
    pub fn is_current(&self, doc: &Document) -> bool {
        self.built_doc.as_ref() == Some(doc)
    }

    /// The script file was just written: the picture and whether it plays are out
    /// of date, whatever a job on its way says, so it must build again.
    pub fn script_changed(&mut self) {
        self.refresh = true;
    }

    /// Whether Play can start on what is up right now. After a live edit it needs
    /// one more preview, as a game collides with what that compiles.
    pub fn playable_now(&self, doc: &Document) -> bool {
        self.is_current(doc) && !self.live_dirty
    }

    /// Render threads still running, including ones told to stop.
    #[cfg(test)]
    pub fn worker_threads(&self) -> usize {
        let retired = self.retired.iter().filter(|h| !h.is_finished()).count();
        usize::from(self.handle.is_some()) + retired
    }

    /// Resets the camera for a new document and forgets the old one.
    pub fn reset(&mut self, doc: &Document) {
        self.forget_build();
        self.empty = false;
        self.mode = Mode::Edit;
        self.camera = FlyCam::for_document(doc);
    }

    /// Drops the picture and everything being prepared for it.
    fn forget_build(&mut self) {
        self.preview_job = None;
        self.bake_job = None;
        self.live_job = None;
        self.live_bsp = None;
        self.warned.clear();
        self.retire_worker();
        self.built_doc = None;
        self.wanted = None;
        self.refresh = false;
        self.light = Light::Draft;
        self.failed_bake = None;
        self.error = None;
        self.play_pending = false;
        self.unplayable = None;
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

    /// The camera the 3D pane draws from, when it can be read without
    /// waiting: while the render thread holds it, `None`, not an older pose.
    pub fn shown_camera(&self) -> Option<FlyCam> {
        match &self.worker {
            Some(w) => w.current_camera(),
            None => Some(self.camera.clone()),
        }
    }

    /// Sets the faces to mark in the 3D view. Nothing is sent when they are
    /// the ones already shown.
    pub fn set_highlights(&mut self, marks: crate::model::facetex::Marks) {
        if *self.highlights == marks {
            return;
        }
        self.highlights = Highlights::new(marks);
        if let Some(w) = &self.worker {
            w.highlights(&self.highlights);
        }
    }

    /// What identifies the full lightmap of the picture.
    pub fn light_key(&self) -> u64 {
        self.light_key
    }

    /// The material table: replacing it updates the picture, keeping the light.
    pub fn materials(&self) -> &SharedMaterials {
        &self.materials
    }

    /// The lightmap cache shared by every build.
    #[cfg(test)]
    pub fn cache(&self) -> SharedCache {
        self.cache.clone()
    }

    /// Points the lightmap cache at the file for `level`: `<level>.msl.lit`
    /// beside a saved level, else the per-user cache directory. Never waits
    /// for a job: the path is applied once the cache is free.
    pub fn set_cache_level(&mut self, level: Option<&Path>) {
        self.point_cache_at(lightcache::path_for(level, "untitled"));
    }

    fn point_cache_at(&mut self, path: Option<PathBuf>) {
        match self.cache.try_lock() {
            Ok(mut cache) => cache.set_path(path),
            Err(TryLockError::Poisoned(p)) => p.into_inner().set_path(path),
            Err(TryLockError::WouldBlock) => self.pending_path = Some(path),
        }
    }

    /// Applies a path that had to wait for a job to let go of the cache.
    fn flush_cache_path(&mut self) {
        if let Some(path) = self.pending_path.take() {
            self.point_cache_at(path);
        }
    }

    /// Save: moves the cache file next to `level` and, when the picture is
    /// fully lit with the geometry of `doc`, writes that bake there; a bake
    /// still to come writes there by itself. Returns a note for the status bar
    /// when a file was written. A job holding the cache for a moment must not
    /// freeze the editor: the file then moves right after it.
    pub fn save_lightmap(&mut self, level: &Path, doc: &Document) -> Option<String> {
        let path = lightcache::path_for(Some(level), "untitled")?;
        let same_level = self
            .built_doc
            .as_ref()
            .is_some_and(|d| d.same_geometry(doc));
        let lit = same_level && self.light == Light::Full;
        let mut cache = match self.cache.try_lock() {
            Ok(c) => c,
            Err(TryLockError::Poisoned(p)) => p.into_inner(),
            Err(TryLockError::WouldBlock) => {
                self.pending_path = Some(Some(path));
                return lit.then(|| "lightmap not cached yet: the cache is busy".to_string());
            }
        };
        // A bake can be done while the picture is still a draft. Nothing else would
        // write it: its own write went to wherever the cache pointed then.
        if !lit && !(same_level && cache.holds(self.light_key)) {
            cache.set_path(Some(path));
            return None;
        }
        match cache.save_to(path.clone()) {
            Ok(true) => Some(format!("lightmap cached in {}", path.display())),
            Ok(false) => None,
            Err(e) => Some(format!("lightmap cache not written: {e}")),
        }
    }

    /// Moves the fly camera.
    pub fn set_camera(&mut self, fly: FlyCam) {
        if let Some(w) = &self.worker {
            w.camera(&fly);
        }
        self.camera = fly;
    }

    /// Enters Play on the level in the picture.
    pub fn enter_play(&mut self) -> Result<(), String> {
        let Some(w) = &self.worker else {
            return Err("the level is not ready yet".into());
        };
        if let Some(why) = &self.unplayable {
            return Err(format!("cannot play: {why}"));
        }
        w.mode(Mode::Play);
        self.mode = Mode::Play;
        gpu_view::set_inactive();
        Ok(())
    }

    /// Back to editing; the document was never touched by Play.
    pub fn leave_play(&mut self) {
        self.mode = Mode::Edit;
        if let Some(w) = &self.worker {
            w.mode(Mode::Edit);
        }
    }

    /// Shows a finished level: on the running render thread, or a new one
    /// when there is none yet.
    fn attach(&mut self, scene: Box<dyn Scene>, doc: Document, unplayable: Option<String>) {
        let delay = worker::slow_ms();
        let scene: Box<dyn Scene> = if delay > 0 {
            Box::new(Slow {
                inner: scene,
                delay: std::time::Duration::from_millis(delay),
            })
        } else {
            scene
        };
        if let Some(w) = &mut self.worker {
            w.scene(scene);
        } else {
            match Worker::spawn(scene, self.camera.clone()) {
                Ok((w, h)) => {
                    w.highlights(&self.highlights);
                    self.worker = Some(w);
                    self.handle = Some(h);
                    self.banner = None;
                }
                Err(e) => {
                    self.built_doc = None;
                    self.error = Some(e);
                    return;
                }
            }
        }
        self.built_doc = Some(doc);
        self.unplayable = unplayable;
    }

    /// Starts a render thread on `scene` as if a fully lit preview of `doc`
    /// had finished (tests drive the thread with their own scenes).
    #[cfg(test)]
    pub fn attach_scene(&mut self, scene: Box<dyn Scene>, doc: &Document) {
        self.wanted = Some(doc.clone());
        self.light = Light::Full;
        self.attach(scene, doc.clone(), None);
    }

    fn retire_worker(&mut self) {
        if let Some(w) = self.worker.take() {
            // The next thread starts where this one left the camera.
            if let Some(c) = w.current_camera() {
                self.camera = c;
            }
            w.stop();
        }
        if let Some(h) = self.handle.take() {
            self.retired.push(h);
        }
    }

    /// Forwards the pane's `events`, brings the picture up to `doc` and
    /// collects what the render thread produced. `pane` is the visible pane's
    /// size in pixels (zero when hidden). Never blocks.
    pub fn tick(&mut self, events: &[Event], pane: (u32, u32), doc: &Document) -> Tick {
        let mut out = Tick::default();
        self.flush_cache_path();
        self.sync_preview(doc, &mut out);
        self.retired.retain(|h| !h.is_finished());
        self.sync_banner(doc);
        out.log.extend(gpu_view::take_report());
        // A GPU has the pixels to spare; on software a large pane costs resolution.
        let max_width = if gpu_view::active() { pane.0 } else { 960 };
        let size = if pane.0 == 0 || pane.1 == 0 {
            (0, 0)
        } else {
            frame_size(pane, max_width)
        };
        let Some(w) = self.worker.as_mut() else {
            return out;
        };
        w.flush();
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

    /// Keeps the note on the picture true.
    fn sync_banner(&mut self, doc: &Document) {
        let Some(banner) = self.preview(doc).banner() else {
            return;
        };
        if banner != self.banner
            && let Some(w) = &self.worker
            && w.banner(banner)
        {
            self.banner = banner;
        }
    }

    /// The render thread died: report it and fall back to editing. The
    /// document is untouched; the next edit starts a fresh thread.
    fn lose_worker(&mut self, why: &str, out: &mut Tick) {
        self.retire_worker();
        self.built_doc = None;
        // Not retried for the same document: a scene that panics would panic
        // again, as often as it could be rebuilt.
        self.error = Some(format!("the render thread failed: {why}"));
        out.left_play = self.mode == Mode::Play;
        self.mode = Mode::Edit;
        out.status = Some(format!("Render thread failed: {why}"));
        out.log.push(format!("ERROR: render thread failed: {why}"));
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
mod cache_lock_tests;
#[cfg(test)]
#[path = "studio/gpu_view_tests.rs"]
mod gpu_view_tests;
#[cfg(test)]
mod large_level_tests;
#[cfg(test)]
mod live_tests;
#[cfg(test)]
mod material_tests;
#[cfg(test)]
mod preview_tests;
#[cfg(test)]
mod race_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod texture_tests;
#[cfg(test)]
pub(crate) mod tick_costs;
#[cfg(test)]
mod vista_tests;
