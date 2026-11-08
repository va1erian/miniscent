//! Turning the document into something the 3D pane can draw, off the UI
//! thread, in two stages. A preview job compiles the level and lights it with
//! the stored lightmap or a draft, in milliseconds, and runs for every edit. A
//! bake job traces the full lightmap, however long that takes, and can be
//! dropped when the level changes. Errors (an invalid brush, a CSG failure, a
//! panic in a job) come back as text and never take the editor down.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, TryRecvError, channel};
use std::sync::{Arc, Mutex, PoisonError};

use miniscent_game::scripts::{self, ScriptFiles, ScriptWatcher};
use miniscent_game::{GameSession, SessionConfig};
use mscent_game::{Arena, LoadError};
use mscent_render::{BakeCache, BakeReport, BuildError, Compiled, LevelRenderer, Source};
use mscent_world::lightmap::LightmapError;
use mscent_world::{Entity, Level, vista_warnings};

use crate::errors;
use crate::materials::SharedMaterials;
use crate::model::Document;

/// The last full lightmap, shared by every job: an edit that leaves faces and
/// lights alone gets it back with no bake. Held only for moments, never for a
/// bake.
pub type SharedCache = Arc<Mutex<BakeCache>>;

/// How good the lighting of a preview is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Light {
    /// Coarse, standing in until the full bake is done.
    Draft,
    /// The full lightmap: what the game ships with.
    Full,
}

/// A compiled and lit level.
pub struct Built {
    /// What the 3D pane and Play use.
    pub product: Product,
    pub light: Light,
    /// Identifies the full lightmap of this level.
    pub light_key: u64,
    /// The generation of the material table it draws with.
    pub materials: u64,
    /// The cache's account of a [`Light::Full`] lightmap.
    pub report: Option<BakeReport>,
    /// Reloads the level's script while it is played, for a saved level.
    pub watch: Option<ScriptWatcher>,
    /// What the level's vistas do wrong, one line each; shown in the log.
    pub warnings: Vec<String>,
}

pub enum Product {
    /// Can be played; the session also owns the renderer.
    Playable(Box<GameSession>),
    /// Renders but cannot be played (no start or no goal); the text says why.
    ViewOnly(Box<LevelRenderer>, Unplayable),
}

/// Why a level cannot be played, and the entity to select for it, by index.
#[derive(Clone, Debug, PartialEq)]
pub struct Unplayable {
    pub why: String,
    pub entity: Option<usize>,
}

impl From<String> for Unplayable {
    fn from(why: String) -> Self {
        Self { why, entity: None }
    }
}

impl Built {
    pub fn renderer(&self) -> &LevelRenderer {
        match &self.product {
            Product::Playable(s) => s.renderer(),
            Product::ViewOnly(r, _) => r,
        }
    }

    pub fn renderer_mut(&mut self) -> &mut LevelRenderer {
        match &mut self.product {
            Product::Playable(s) => s.renderer_mut(),
            Product::ViewOnly(r, _) => r,
        }
    }

    pub fn into_renderer(self) -> LevelRenderer {
        match self.product {
            Product::Playable(s) => s.into_renderer(),
            Product::ViewOnly(r, _) => *r,
        }
    }

    pub fn session_mut(&mut self) -> Option<&mut GameSession> {
        match &mut self.product {
            Product::Playable(s) => Some(s),
            Product::ViewOnly(..) => None,
        }
    }

    /// Why the level cannot be played, if it cannot.
    #[cfg(test)]
    pub fn unplayable(&self) -> Option<&str> {
        self.problem().map(|u| u.why.as_str())
    }

    /// As [`Built::unplayable`], with the entity to blame.
    pub fn problem(&self) -> Option<&Unplayable> {
        match &self.product {
            Product::Playable(_) => None,
            Product::ViewOnly(_, why) => Some(why),
        }
    }
}

/// A job running on its own thread.
pub struct Job<T> {
    rx: Receiver<Result<T, String>>,
    /// The document the job works on.
    pub doc: Document,
}

impl<T> Job<T> {
    /// The result, once there is one. A thread that vanished without one is
    /// reported as a failure.
    pub fn poll(&self) -> Option<Result<T, String>> {
        match self.rx.try_recv() {
            Ok(r) => Some(r),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => Some(Err("the build thread stopped".into())),
        }
    }
}

/// A full bake in progress. Dropping it cancels the bake.
pub struct BakeJob {
    /// The report of the finished bake; `None` when it was cancelled.
    pub job: Job<Option<BakeReport>>,
    /// The lightmap being baked, as in [`Built::light_key`].
    pub key: u64,
    cancel: Arc<AtomicBool>,
}

impl Drop for BakeJob {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

/// Checks every brush and the entities, naming the first problem. The
/// editor can then show "brush 7 (id 12) is invalid" instead of a code.
pub fn validate(doc: &Document) -> Result<Level, String> {
    if doc.brushes().is_empty() {
        return Err("nothing to build: the level has no brushes".into());
    }
    let (mut brushes, mut volumes) = (Vec::new(), Vec::new());
    for (i, def) in doc.brushes().iter().enumerate() {
        if let Some(v) = def.volume() {
            volumes.push(v.clone().with_order(i as u32));
            continue;
        }
        let brush = def
            .to_brush(i as u32)
            .map_err(|e| format!("brush {} is invalid: {}", i + 1, errors::brush(e)))?;
        brushes.push(brush);
    }
    let entities: Vec<Entity> = doc.entities().iter().map(|e| e.entity).collect();
    if let Some(i) = entities.iter().position(|e| !e.is_valid()) {
        return Err(format!("entity {} has an out-of-range value", i + 1));
    }
    let level = Level {
        brushes,
        entities,
        volumes,
        labels: doc.export_labels(),
        mover_groups: doc.export_groups(),
    };
    level.check_labels().map_err(errors::label)?;
    Ok(level)
}

/// Runs `work` on `doc` on a new thread; a panic becomes an error text.
pub(crate) fn spawn<T: Send + 'static>(
    name: &str,
    doc: &Document,
    work: impl FnOnce(&Document) -> Result<T, String> + Send + 'static,
) -> Result<Job<T>, String> {
    let (tx, rx) = channel();
    let copy = doc.clone();
    std::thread::Builder::new()
        .name(name.into())
        .spawn(move || {
            let result = catch_unwind(AssertUnwindSafe(|| work(&copy)))
                .unwrap_or_else(|p| Err(format!("the build crashed: {}", panic_text(&*p))));
            let _ = tx.send(result);
        })
        .map_err(|e| format!("cannot start the build thread: {e}"))?;
    Ok(Job {
        rx,
        doc: doc.clone(),
    })
}

pub(crate) fn panic_text(p: &(dyn std::any::Any + Send)) -> String {
    p.downcast_ref::<&str>()
        .map(|s| (*s).to_string())
        .or_else(|| p.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "unknown panic".into())
}

/// Starts the job that makes `doc` visible as fast as possible.
pub fn start_preview(
    doc: &Document,
    cache: &SharedCache,
    materials: &SharedMaterials,
    cfg: &SessionConfig,
) -> Result<Job<Built>, String> {
    let (cache, materials, cfg) = (Arc::clone(cache), Arc::clone(materials), *cfg);
    spawn("mscent-preview", doc, move |doc| {
        preview(doc, &cache, &materials, &cfg)
    })
}

/// Starts the full bake of `doc`, whose [`Built::light_key`] is `key`.
pub fn start_bake(
    doc: &Document,
    key: u64,
    cache: &SharedCache,
    cfg: &SessionConfig,
) -> Result<BakeJob, String> {
    let (cache, cfg) = (Arc::clone(cache), *cfg);
    let cancel = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&cancel);
    let job = spawn("mscent-bake", doc, move |doc| {
        bake(doc, &cache, &cfg, &flag)
    })?;
    Ok(BakeJob { job, key, cancel })
}

fn lock(cache: &SharedCache) -> std::sync::MutexGuard<'_, BakeCache> {
    cache.lock().unwrap_or_else(PoisonError::into_inner)
}

fn compile_level(level: &Level, cfg: &SessionConfig) -> Result<Compiled, String> {
    Compiled::new(level, &cfg.build).map_err(errors::build)
}

fn preview(
    doc: &Document,
    cache: &SharedCache,
    materials: &SharedMaterials,
    cfg: &SessionConfig,
) -> Result<Built, String> {
    let level = validate(doc)?;
    // Before the script is read, so an edit made meanwhile is seen as a change.
    let watch = doc.level_path().map(ScriptWatcher::new);
    // The game's own copy of the level is built beside the picture: on a voxel
    // level it costs as much as the CSG.
    std::thread::scope(|s| {
        let rules = s.spawn(|| rules(&level, doc.level_path()));
        let picture = picture(&level, cache, materials, cfg);
        let rules = rules
            .join()
            .unwrap_or_else(|p| Err(format!("the build crashed: {}", panic_text(&*p))));
        let (renderer, lit) = picture?;
        Ok(Built {
            product: product(rules, renderer, cfg, &level.entities),
            light: lit.light,
            light_key: lit.light_key,
            materials: lit.materials,
            report: lit.report,
            watch,
            warnings: lit.warnings,
        })
    })
}

/// What a preview knows of a level besides its renderer.
struct Lit {
    light: Light,
    light_key: u64,
    materials: u64,
    report: Option<BakeReport>,
    warnings: Vec<String>,
}

/// The level's renderer with the best light at hand.
fn picture(
    level: &Level,
    cache: &SharedCache,
    materials: &SharedMaterials,
    cfg: &SessionConfig,
) -> Result<(LevelRenderer, Lit), String> {
    let compiled = compile_level(level, cfg)?;
    let light_key = compiled.light_key();
    let stored = {
        let mut cache = lock(cache);
        let map = compiled.stored(&mut cache);
        map.map(|m| (m, cache.last_report().cloned()))
    };
    let (map, light, report) = match stored {
        Some((map, report)) => (map, Light::Full, report),
        None => {
            let draft = compiled.draft().map_err(errors::build)?;
            (Arc::new(draft), Light::Draft, None)
        }
    };
    // One load for the whole session: the stock materials are generated, about 4 ms
    // each on a size-optimized build, which a preview should not pay again. The
    // table's clone shares its pixels and mips.
    let (generation, table) = materials.current();
    let renderer = compiled.into_renderer(map, table?).map_err(errors::build)?;
    let warnings = vista_warnings(level)
        .iter()
        .map(ToString::to_string)
        .collect();
    let lit = Lit {
        light,
        light_key,
        materials: generation,
        report,
        warnings,
    };
    Ok((renderer, lit))
}

/// A session on the level's script, or why there cannot be one. The renderer
/// comes back either way, so a broken script still leaves a picture to look at.
fn scripted(
    arena: Arena,
    renderer: LevelRenderer,
    cfg: &SessionConfig,
    files: &ScriptFiles,
) -> Product {
    let mut session = GameSession::with_renderer(arena, renderer, cfg);
    match scripts::start(&mut session, files) {
        Ok(_warnings) => Product::Playable(Box::new(session)),
        Err(why) => Product::ViewOnly(Box::new(session.into_renderer()), why.into()),
    }
}

/// The level's scripts and the arena the game plays it in, or why the script
/// cannot be read.
type Rules = Result<(Option<ScriptFiles>, Result<Arena, LoadError>), String>;

fn rules(level: &Level, path: Option<&Path>) -> Rules {
    let files = path.map(scripts::for_level).transpose()?.flatten();
    let arena = if files.is_some() {
        Arena::from_scripted_level(level)
    } else {
        Arena::from_level(level)
    };
    Ok((files, arena))
}

fn product(rules: Rules, renderer: LevelRenderer, cfg: &SessionConfig, ents: &[Entity]) -> Product {
    let (files, arena) = match rules {
        Ok(rules) => rules,
        Err(why) => return Product::ViewOnly(Box::new(renderer), why.into()),
    };
    match arena {
        Ok(arena) => match &files {
            Some(files) => scripted(arena, renderer, cfg, files),
            None => Product::Playable(Box::new(GameSession::with_renderer(arena, renderer, cfg))),
        },
        Err(e) => {
            let (why, entity) = errors::load(e, ents);
            Product::ViewOnly(Box::new(renderer), Unplayable { why, entity })
        }
    }
}

/// All cores but one: the render and UI threads keep running during a bake.
fn bake_workers() -> usize {
    std::thread::available_parallelism()
        .map_or(1, usize::from)
        .saturating_sub(1)
        .max(1)
}

fn bake(
    doc: &Document,
    cache: &SharedCache,
    cfg: &SessionConfig,
    cancel: &AtomicBool,
) -> Result<Option<BakeReport>, String> {
    let level = validate(doc)?;
    let compiled = compile_level(&level, cfg)?;
    // Looked up first so the report can say why a file was not good enough.
    if compiled.stored(&mut lock(cache)).is_some() {
        return Ok(lock(cache).last_report().cloned());
    }
    // The cache stays free for the whole bake: previews and Save go on.
    let baked = match compiled.bake(bake_workers(), cancel) {
        Ok(b) => b,
        Err(BuildError::Bake(LightmapError::Cancelled)) => return Ok(None),
        Err(e) => return Err(errors::build(e)),
    };
    if cancel.load(Ordering::Relaxed) {
        return Ok(None);
    }
    let mut cache = lock(cache);
    cache.store(baked);
    Ok(cache.last_report().cloned())
}

/// A report as one sentence for the log.
pub fn log_line(report: &BakeReport) -> String {
    let mut line = report.describe();
    if let Some(first) = line.get_mut(..1) {
        first.make_ascii_uppercase();
    }
    line
}

/// Whether a preview's lightmap came from the file, which the log mentions; a
/// memory hit happens on every edit and is not news.
pub fn loaded_from_file(built: &Built) -> bool {
    built.report.as_ref().map(|r| r.source) == Some(Source::File)
}

#[cfg(test)]
#[path = "build_tests.rs"]
mod tests;
