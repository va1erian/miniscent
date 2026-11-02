//! Turning the document into something the 3D pane can draw, off the UI
//! thread, in two stages. A preview job compiles the level and lights it with
//! the stored lightmap or a draft, in milliseconds, and runs for every edit. A
//! bake job traces the full lightmap, however long that takes, and can be
//! dropped when the level changes. Errors (an invalid brush, a CSG failure, a
//! panic in a job) come back as text and never take the editor down.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, TryRecvError, channel};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};

use miniscent_game::assets::load_materials;
use miniscent_game::{GameSession, SessionConfig};
use mscent_game::{Arena, LoadError};
use mscent_render::{
    BakeCache, BakeReport, BuildError, Compiled, LevelRenderer, MaterialTable, Source,
};
use mscent_world::lightmap::LightmapError;
use mscent_world::{Entity, Level};

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
    /// The cache's account of a [`Light::Full`] lightmap.
    pub report: Option<BakeReport>,
}

pub enum Product {
    /// Can be played; the session also owns the renderer.
    Playable(Box<GameSession>),
    /// Renders but cannot be played (no start or no goal); the text says why.
    ViewOnly(Box<LevelRenderer>, String),
}

impl Built {
    pub fn renderer(&self) -> &LevelRenderer {
        match &self.product {
            Product::Playable(s) => s.renderer(),
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
    pub fn unplayable(&self) -> Option<&str> {
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
    let orders = doc.csg_orders();
    let mut brushes = Vec::with_capacity(doc.brushes().len());
    for (i, def) in doc.brushes().iter().enumerate() {
        let brush = def
            .to_brush(orders.brushes[i])
            .map_err(|e| format!("brush {} (id {}) is invalid: {e:?}", i + 1, def.id))?;
        brushes.push(brush);
    }
    let entities: Vec<Entity> = doc.entities().iter().map(|e| e.entity).collect();
    if let Some(i) = entities.iter().position(|e| !e.is_valid()) {
        return Err(format!("entity {} has an out-of-range value", i + 1));
    }
    Ok(Level {
        brushes,
        entities,
        volumes: orders.volumes,
    })
}

/// Runs `work` on `doc` on a new thread; a panic becomes an error text.
fn spawn<T: Send + 'static>(
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
pub fn start_preview(doc: &Document, cache: &SharedCache) -> Result<Job<Built>, String> {
    let cache = Arc::clone(cache);
    spawn("mscent-preview", doc, move |doc| preview(doc, &cache))
}

/// Starts the full bake of `doc`, whose [`Built::light_key`] is `key`.
pub fn start_bake(doc: &Document, key: u64, cache: &SharedCache) -> Result<BakeJob, String> {
    let cache = Arc::clone(cache);
    let cancel = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&cancel);
    let job = spawn("mscent-bake", doc, move |doc| bake(doc, &cache, &flag))?;
    Ok(BakeJob { job, key, cancel })
}

/// One decode for the whole session: at 67 ms it would be most of the cost of
/// a preview, and a clone shares the pixels.
fn materials() -> Result<MaterialTable, String> {
    static TABLE: OnceLock<Result<MaterialTable, String>> = OnceLock::new();
    TABLE.get_or_init(|| load_materials(None)).clone()
}

fn lock(cache: &SharedCache) -> std::sync::MutexGuard<'_, BakeCache> {
    cache.lock().unwrap_or_else(PoisonError::into_inner)
}

fn compile_level(level: &Level, cfg: &SessionConfig) -> Result<Compiled, String> {
    Compiled::new(level, &cfg.build).map_err(|e| format!("CSG failed: {e:?}"))
}

fn preview(doc: &Document, cache: &SharedCache) -> Result<Built, String> {
    let level = validate(doc)?;
    let cfg = SessionConfig::default();
    let compiled = compile_level(&level, &cfg)?;
    let light_key = compiled.light_key();
    let stored = {
        let mut cache = lock(cache);
        let map = compiled.stored(&mut cache);
        map.map(|m| (m, cache.last_report().cloned()))
    };
    let (map, light, report) = match stored {
        Some((map, report)) => (map, Light::Full, report),
        None => {
            let draft = compiled
                .draft()
                .map_err(|e| format!("draft failed: {e:?}"))?;
            (Arc::new(draft), Light::Draft, None)
        }
    };
    let renderer = compiled
        .into_renderer(map, materials()?)
        .map_err(|e| format!("cannot prepare the level: {e:?}"))?;
    Ok(Built {
        product: product(&level, renderer, &cfg),
        light,
        light_key,
        report,
    })
}

fn product(level: &Level, renderer: LevelRenderer, cfg: &SessionConfig) -> Product {
    match Arena::from_level(level) {
        Ok(arena) => Product::Playable(Box::new(GameSession::with_renderer(arena, renderer, cfg))),
        Err(LoadError::NoPlayerStart) => {
            Product::ViewOnly(Box::new(renderer), "the level has no player start".into())
        }
        Err(LoadError::NoGoal) => {
            Product::ViewOnly(Box::new(renderer), "the level has no goal".into())
        }
        Err(other) => Product::ViewOnly(
            Box::new(renderer),
            format!("the level cannot be played ({other:?})"),
        ),
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
    cancel: &AtomicBool,
) -> Result<Option<BakeReport>, String> {
    let level = validate(doc)?;
    let compiled = compile_level(&level, &SessionConfig::default())?;
    // Looked up first so the report can say why a file was not good enough.
    if compiled.stored(&mut lock(cache)).is_some() {
        return Ok(lock(cache).last_report().cloned());
    }
    // The cache stays free for the whole bake: previews and Save go on.
    let baked = match compiled.bake(bake_workers(), cancel) {
        Ok(b) => b,
        Err(BuildError::Bake(LightmapError::Cancelled)) => return Ok(None),
        Err(e) => return Err(format!("bake failed: {e:?}")),
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
mod tests {
    use super::*;
    use mscent_world::Op;

    fn wait<T>(job: &Job<T>) -> Result<T, String> {
        loop {
            if let Some(r) = job.poll() {
                return r;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }

    #[test]
    fn invalid_brushes_and_empty_levels_are_errors_that_name_the_problem() {
        assert!(
            validate(&Document::new())
                .unwrap_err()
                .contains("no brushes")
        );
        let mut state =
            crate::model::EditorState::new(Document::from_level(&mscent_world::showcase_level()));
        // A zero-size box is refused by the model, so push one in through
        // the document's own replace path to model a corrupt document.
        state.add_box([0.0; 3], [1.0, 1.0, 1.0]).expect("added");
        let id = state.doc.brushes().last().expect("brush").id;
        let mut def = state.doc.brush(id).expect("alive").clone();
        def.shape = crate::model::document::Shape::Box {
            min: [1.0, 1.0, 1.0],
            max: [1.0, 1.0, 1.0],
        };
        let mut doc = state.doc.clone();
        doc.replace_brush_for_test(&def);
        let err = validate(&doc).unwrap_err();
        assert!(err.contains("is invalid"), "{err}");
    }

    #[test]
    fn a_level_without_start_or_goal_previews_view_only() {
        let room = mscent_world::Brush::cuboid(
            mscent_math::Vec3d::new(-4.0, -4.0, -1.0),
            mscent_math::Vec3d::new(4.0, 4.0, 3.0),
            Op::Subtract,
            0,
        )
        .expect("room");
        let doc = Document::from_level(&Level {
            brushes: vec![room],
            entities: vec![],
            volumes: Vec::new(),
        });
        let job = start_preview(&doc, &SharedCache::default()).expect("starts");
        let built = wait(&job).expect("builds");
        assert!(built.unplayable().is_some());
    }

    #[test]
    fn the_showcase_previews_playable_on_a_draft_and_a_bake_makes_it_full() {
        let doc = Document::from_level(&mscent_world::showcase_level());
        let cache = SharedCache::default();
        let first = wait(&start_preview(&doc, &cache).expect("starts")).expect("builds");
        assert!(first.unplayable().is_none());
        assert_eq!(first.light, Light::Draft);
        assert_eq!(
            cache.lock().expect("cache").bakes(),
            0,
            "a draft is no bake"
        );

        let job = start_bake(&doc, first.light_key, &cache).expect("starts");
        let report = wait(&job.job).expect("bakes").expect("not cancelled");
        assert_eq!(report.source, Source::Baked);

        let second = wait(&start_preview(&doc, &cache).expect("starts")).expect("builds");
        assert_eq!(second.light, Light::Full);
        assert_eq!(second.light_key, first.light_key);
        assert_eq!(cache.lock().expect("cache").bakes(), 1);
    }

    #[test]
    fn dropping_a_bake_job_cancels_it() {
        let doc = Document::from_level(&mscent_world::showcase_level());
        let cache = SharedCache::default();
        let cancel = Arc::new(AtomicBool::new(true));
        let flag = Arc::clone(&cancel);
        let copy = Arc::clone(&cache);
        let job = spawn("test-bake", &doc, move |d| bake(d, &copy, &flag)).expect("starts");
        assert!(wait(&job).expect("runs").is_none(), "reported as cancelled");
        assert_eq!(cache.lock().expect("cache").bakes(), 0, "nothing stored");

        let job = start_bake(&doc, 0, &cache).expect("starts");
        let flag = Arc::clone(&job.cancel);
        drop(job);
        assert!(flag.load(Ordering::Relaxed));
    }
}
