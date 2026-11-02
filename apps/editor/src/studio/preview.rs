//! Keeps the 3D pane in step with the document with no command from the user.
//! Every tick compares the document with the one last sent to a preview job;
//! a difference starts the next job, so a long drag costs one job at a time and
//! the newest document always wins. A preview shows a stored lightmap or a
//! draft. Once the edits stop, a full bake runs in the background; it does not
//! block anything, and an edit that changes the lighting drops it.

use std::time::{Duration, Instant};

use miniscent_game::scripts::ScriptWatcher;

use super::scene::LevelScene;
use super::{HELP, Mode, Studio, Tick};
use crate::build::{self, Built, Light};
use crate::model::Document;

/// How long the document must stay put before a full bake starts: a drag sends
/// many edits, and each would only cancel the bake of the one before.
const QUIET_MS: u128 = 150;

/// The 3D pane's text when the level has nothing to draw.
fn empty_text() -> String {
    format!("The level is empty: draw a brush to see it here.\n\n{HELP}")
}

/// How far the picture has caught up with the document.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Preview {
    /// Nothing to draw: the level has no brushes.
    Empty,
    /// A preview of the current document is still on its way.
    Updating,
    /// The picture is current, on draft lighting; the full bake is coming.
    Baking,
    /// The picture is current on draft lighting because the full bake failed.
    DraftOnly,
    /// The picture is current and fully lit.
    Full,
    /// The document cannot be drawn; the picture shows an earlier state.
    Failed,
}

impl Preview {
    /// For the status bar.
    pub fn label(self) -> &'static str {
        match self {
            Preview::Empty => "",
            Preview::Updating => "Updating preview...",
            Preview::Baking => "Baking lighting...",
            Preview::DraftOnly => "Draft lighting: bake failed",
            Preview::Full => "Lighting up to date",
            Preview::Failed => "Preview not updated",
        }
    }

    /// What to write on the picture. `None`: leave it as it is, so a picture
    /// about to be replaced does not flicker.
    pub(super) fn banner(self) -> Option<Option<&'static str>> {
        match self {
            Preview::Empty | Preview::Updating => None,
            Preview::Full => Some(None),
            Preview::Baking => Some(Some("BAKING LIGHT")),
            Preview::DraftOnly => Some(Some("DRAFT LIGHT")),
            Preview::Failed => Some(Some("NOT UPDATED")),
        }
    }
}

/// How often the script beside the level is looked at while editing.
const SCRIPT_POLL: Duration = Duration::from_millis(500);

/// The scripts of the level in the picture, as they were when it was built.
pub(super) struct ScriptWatch {
    files: Option<ScriptWatcher>,
    checked: Instant,
}

impl Default for ScriptWatch {
    fn default() -> Self {
        Self {
            files: None,
            checked: Instant::now(),
        }
    }
}

impl Studio {
    /// Where the picture stands against `doc`.
    pub fn preview(&self, doc: &Document) -> Preview {
        if doc.brushes().is_empty() {
            Preview::Empty
        } else if self.error.is_some() {
            Preview::Failed
        } else if self.built_doc.as_ref() != Some(doc)
            || self.built_materials != self.materials_seen
        {
            Preview::Updating
        } else if self.light == Light::Full {
            Preview::Full
        } else if self.failed_bake == Some(self.light_key) {
            Preview::DraftOnly
        } else {
            Preview::Baking
        }
    }

    /// Plays `doc` as soon as a preview of it is up. Errors say why it will
    /// not be.
    pub fn play_when_ready(&mut self, doc: &Document) -> Result<(), String> {
        if doc.brushes().is_empty() {
            return Err("Nothing to play: the level has no brushes".into());
        }
        if let (Some(e), None) = (&self.error, &self.preview_job) {
            return Err(format!("Cannot play: {e}"));
        }
        self.play_pending = true;
        self.refresh |= self.live_dirty;
        Ok(())
    }

    /// One tick's worth of keeping up with `doc`. Never blocks.
    pub(super) fn sync_preview(&mut self, doc: &Document, out: &mut Tick) {
        self.poll_preview(doc, out);
        self.poll_bake(out);
        if doc.brushes().is_empty() {
            self.show_empty(doc, out);
            return;
        }
        self.empty = false;
        self.poll_live(out);
        self.watch_script();
        self.watch_materials();
        let changed = self.wanted.as_ref() != Some(doc);
        if changed {
            self.changed_at = Instant::now();
        }
        if changed && self.mode == Mode::Edit && self.try_live(doc) {
            return;
        }
        if self.live_job.is_some() {
            return;
        }
        self.start_preview(doc, changed, out);
        self.start_bake(doc, out);
    }

    /// A script edited outside the editor decides whether the level plays, so
    /// while editing a change builds it again. During Play the scene reloads it.
    fn watch_script(&mut self) {
        if self.mode != Mode::Edit || self.script_watch.checked.elapsed() < SCRIPT_POLL {
            return;
        }
        self.script_watch.checked = Instant::now();
        let files = self.script_watch.files.as_mut();
        self.refresh |= files.is_some_and(ScriptWatcher::changed);
    }

    /// A new generation of the material table builds the picture again, as a
    /// finished bake does: same document, so the same lightmap comes back.
    fn watch_materials(&mut self) {
        let generation = self.materials.generation();
        if generation != self.materials_seen {
            self.materials_seen = generation;
            self.refresh = true;
            // The table that failed is gone; this one has not been tried.
            self.error = None;
        }
    }

    /// An empty level draws nothing, so the last picture must go; whatever is
    /// still being prepared is for a level that no longer exists.
    fn show_empty(&mut self, doc: &Document, out: &mut Tick) {
        if self.empty {
            return;
        }
        self.empty = true;
        self.forget_build();
        self.wanted = Some(doc.clone());
        out.blank = Some(empty_text());
    }

    /// In Play the document is frozen and the running game is not to be
    /// replaced: only a finished bake's lighting is fetched.
    fn start_preview(&mut self, doc: &Document, changed: bool, out: &mut Tick) {
        let wanted = (changed && self.mode == Mode::Edit) || self.refresh;
        if self.preview_job.is_some() || !wanted || self.stroking {
            return;
        }
        self.refresh = false;
        self.flush_cache_path();
        self.wanted = Some(doc.clone());
        match build::start_preview(doc, &self.cache, &self.materials, &self.settings) {
            Ok(job) => self.preview_job = Some(job),
            Err(e) => self.preview_failed(e, doc, doc, out),
        }
    }

    fn start_bake(&mut self, doc: &Document, out: &mut Tick) {
        let waiting = self.bake_job.is_some()
            || self.preview_job.is_some()
            || self.refresh
            || self.stroking
            // The bake's end would ask for the preview that just failed, and
            // that would bake again. A new document or table clears this.
            || self.error.is_some()
            || self.worker.is_none()
            || self.light == Light::Full
            || self.failed_bake == Some(self.light_key)
            || self.built_doc.as_ref() != Some(doc)
            || self.changed_at.elapsed().as_millis() < QUIET_MS;
        if waiting {
            return;
        }
        match build::start_bake(doc, self.light_key, &self.cache, &self.settings) {
            Ok(job) => self.bake_job = Some(job),
            Err(e) => self.bake_failed(e, out),
        }
    }

    fn poll_preview(&mut self, doc: &Document, out: &mut Tick) {
        let Some(result) = self.preview_job.as_ref().and_then(|j| j.poll()) else {
            return;
        };
        let made_for = self.preview_job.take().map(|j| j.doc).unwrap_or_default();
        match result {
            Ok(built) => self.adopt(built, made_for, doc, out),
            Err(e) => self.preview_failed(e, &made_for, doc, out),
        }
    }

    /// A preview is ready. An older document than the current one is still
    /// shown: during a long drag the picture follows it in steps.
    fn adopt(&mut self, built: Built, made_for: Document, doc: &Document, out: &mut Tick) {
        if build::loaded_from_file(&built)
            && let Some(report) = &built.report
        {
            out.log.push(build::log_line(report));
        }
        if self.mode == Mode::Play {
            self.relight(built, &made_for);
            return;
        }
        if self
            .bake_job
            .as_ref()
            .is_some_and(|b| b.key != built.light_key)
        {
            self.bake_job = None;
        }
        self.error = None;
        self.light = built.light;
        self.light_key = built.light_key;
        self.live_dirty = false;
        self.live_bsp = Some(std::sync::Arc::new(built.renderer().scene().bsp().clone()));
        self.built_materials = built.materials;
        let why = built.unplayable().map(str::to_string);
        self.script_watch.files = built.watch.clone();
        self.attach(Box::new(LevelScene::new(built)), made_for, why);
        // Not while another build is wanted: this one may have read an older script.
        if self.play_pending && !self.refresh && self.built_doc.as_ref() == Some(doc) {
            self.play_pending = false;
            match &self.unplayable {
                None => out.play_now = true,
                Some(why) => out.status = Some(format!("Cannot play: {why}")),
            }
        }
    }

    /// While playing, a full lightmap of the level in play replaces the draft
    /// under the running game. Anything else waits until Play is over.
    fn relight(&mut self, built: Built, made_for: &Document) {
        let same_level = self.built_doc.as_ref() == Some(made_for);
        match &mut self.worker {
            Some(w) if same_level && built.light == Light::Full => {
                self.light = Light::Full;
                self.built_materials = built.materials;
                w.relight(built);
            }
            _ => self.wanted = None,
        }
    }

    fn preview_failed(&mut self, e: String, made_for: &Document, doc: &Document, out: &mut Tick) {
        // A newer document is on its way and may well be fine.
        if made_for != doc {
            return;
        }
        if std::mem::take(&mut self.play_pending) {
            out.status = Some(format!("Cannot play: {e}"));
        } else {
            out.status = Some(format!("Preview not updated: {e}"));
        }
        if self.error.as_deref() != Some(e.as_str()) {
            out.log.push(format!("ERROR: {e}"));
        }
        if self.worker.is_none() {
            out.blank = Some(format!("This level cannot be shown:\n{e}"));
        }
        self.error = Some(e);
    }

    fn poll_bake(&mut self, out: &mut Tick) {
        let Some(result) = self.bake_job.as_ref().and_then(|b| b.job.poll()) else {
            return;
        };
        self.bake_job = None;
        match result {
            Ok(Some(report)) => {
                out.log.push(build::log_line(&report));
                // The next preview finds the lightmap in the cache. While the
                // level cannot be shown, whatever clears that makes one anyway.
                self.refresh |= self.error.is_none();
            }
            Ok(None) => {}
            Err(e) => self.bake_failed(e, out),
        }
    }

    /// The draft stays up; the same lighting is not tried again until the
    /// level changes.
    fn bake_failed(&mut self, e: String, out: &mut Tick) {
        self.failed_bake = Some(self.light_key);
        out.status = Some(format!("Keeping draft lighting: {e}"));
        out.log.push(format!("ERROR: {e}"));
    }
}
