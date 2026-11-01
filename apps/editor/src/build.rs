//! The Build command: validation, CSG, faces, portals and the lightmap bake
//! on a worker thread, reported as progress messages. Errors (an invalid
//! brush, a CSG failure, a panic in the worker) come back as text and never
//! take the editor down.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Instant;

use miniscent_game::assets::load_materials;
use miniscent_game::{GameSession, SessionConfig};
use mscent_game::{Arena, LoadError};
use mscent_render::{BakeCache, BakeReport, BuildConfig, LevelRenderer, Source};
use mscent_world::{Entity, Level, Portals, compile, extract_faces};

use crate::model::Document;

/// Number of progress steps a build reports.
pub const STEPS: u32 = 6;

/// The last lightmap, shared by every build so a rebuild that changed only
/// texture mappings does not bake again.
pub type SharedCache = Arc<Mutex<BakeCache>>;

/// A compiled and baked level.
pub struct Built {
    /// What the 3D pane and Play use.
    pub product: Product,
    /// Where the lightmap came from: a bake, or the cache.
    pub bake: Option<Source>,
    /// The cache's account of it: timing, file, why a file was ignored.
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

/// What the worker tells the UI.
pub enum Update {
    Step { step: u32, text: String },
    Finished(Result<Built, String>),
}

/// A running build.
pub struct BuildJob {
    rx: Receiver<Update>,
    /// The document being built, to tell later whether the result is stale.
    pub doc: Document,
    /// Start playing as soon as the build succeeds.
    pub then_play: bool,
    pub started: Instant,
    /// A background refresh (texture edits): it does not write the log.
    pub quiet: bool,
}

/// Checks every brush and the entities, naming the first problem. The
/// editor can then show "brush 7 (id 12) is invalid" instead of a code.
pub fn validate(doc: &Document) -> Result<Level, String> {
    if doc.brushes().is_empty() {
        return Err("nothing to build: the level has no brushes".into());
    }
    let mut brushes = Vec::with_capacity(doc.brushes().len());
    for (i, def) in doc.brushes().iter().enumerate() {
        let brush = def
            .to_brush(i as u32)
            .map_err(|e| format!("brush {} (id {}) is invalid: {e:?}", i + 1, def.id))?;
        brushes.push(brush);
    }
    let entities: Vec<Entity> = doc.entities().iter().map(|e| e.entity).collect();
    if let Some(i) = entities.iter().position(|e| !e.is_valid()) {
        return Err(format!("entity {} has an out-of-range value", i + 1));
    }
    Ok(Level { brushes, entities })
}

/// Starts a build of `doc` on a worker thread, baking through `cache`.
pub fn start_cached(
    doc: &Document,
    then_play: bool,
    cache: &SharedCache,
) -> Result<BuildJob, String> {
    let level = validate(doc)?;
    let (tx, rx) = channel();
    let tx2 = tx.clone();
    let cache = Arc::clone(cache);
    std::thread::Builder::new()
        .name("mscent-build".into())
        .spawn(move || {
            let result = catch_unwind(AssertUnwindSafe(|| work(&level, &tx, &cache)))
                .unwrap_or_else(|p| Err(format!("the build crashed: {}", panic_text(&*p))));
            let _ = tx2.send(Update::Finished(result));
        })
        .map_err(|e| format!("cannot start the build thread: {e}"))?;
    Ok(BuildJob {
        rx,
        doc: doc.clone(),
        then_play,
        started: Instant::now(),
        quiet: false,
    })
}

pub(crate) fn panic_text(p: &(dyn std::any::Any + Send)) -> String {
    p.downcast_ref::<&str>()
        .map(|s| (*s).to_string())
        .or_else(|| p.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "unknown panic".into())
}

impl BuildJob {
    /// Everything the worker has reported since the last call. A worker that
    /// vanished without a result is reported as a failure.
    pub fn poll(&mut self) -> Vec<Update> {
        let mut out = Vec::new();
        loop {
            match self.rx.try_recv() {
                Ok(u) => out.push(u),
                Err(std::sync::mpsc::TryRecvError::Empty) => return out,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    if !out.iter().any(|u| matches!(u, Update::Finished(_))) {
                        out.push(Update::Finished(Err("the build thread stopped".into())));
                    }
                    return out;
                }
            }
        }
    }
}

fn work(level: &Level, tx: &Sender<Update>, cache: &SharedCache) -> Result<Built, String> {
    let t0 = Instant::now();
    let note = |step: u32, text: String| {
        let _ = tx.send(Update::Step { step, text });
    };
    note(
        1,
        format!(
            "Checked {} brushes and {} entities",
            level.brushes.len(),
            level.entities.len()
        ),
    );
    let t = Instant::now();
    let bsp = compile(&level.brushes).map_err(|e| format!("CSG failed: {e:?}"))?;
    note(
        2,
        format!(
            "CSG: {} nodes, {} leaves, {} ms",
            bsp.nodes().len(),
            bsp.leaves().len(),
            t.elapsed().as_millis()
        ),
    );
    let faces = extract_faces(&bsp, &level.brushes);
    note(3, format!("Faces: {}", faces.len()));
    let portals = Portals::build(&bsp);
    note(4, format!("Portals: {}", portals.portals().len()));
    drop((faces, portals, bsp));
    let materials = load_materials(None)?;
    note(5, "Lightmap: using the cache, baking only if needed".into());
    let cfg = SessionConfig::default();
    let mut cache = cache.lock().unwrap_or_else(PoisonError::into_inner);
    let product = match Arena::from_level(level) {
        Ok(_) => Product::Playable(Box::new(
            GameSession::new_cached(level, materials, &cfg, &mut cache)
                .map_err(|e| e.to_string())?,
        )),
        Err(e) => {
            let why = match e {
                LoadError::NoPlayerStart => "the level has no player start".to_string(),
                LoadError::NoGoal => "the level has no goal".to_string(),
                other => format!("the level cannot be played ({other:?})"),
            };
            let renderer = LevelRenderer::from_level_cached(
                level,
                materials,
                &BuildConfig::default(),
                &mut cache,
            )
            .map_err(|e| format!("bake failed: {e:?}"))?;
            Product::ViewOnly(Box::new(renderer), why)
        }
    };
    let report = cache.last_report().cloned();
    let bake = report.as_ref().map(|r| r.source);
    drop(cache);
    let lit = match &product {
        Product::Playable(s) => s.renderer().lightmap().projections().len(),
        Product::ViewOnly(r, _) => r.lightmap().projections().len(),
    };
    let mut how = report
        .as_ref()
        .map_or_else(|| "lightmap baked".into(), BakeReport::describe);
    how[..1].make_ascii_uppercase();
    note(
        6,
        format!(
            "{how}: {lit} lit faces. Build finished in {} ms",
            t0.elapsed().as_millis()
        ),
    );
    if let Product::ViewOnly(_, why) = &product {
        let _ = tx.send(Update::Step {
            step: STEPS,
            text: format!("Not playable: {why}"),
        });
    }
    Ok(Built {
        product,
        bake,
        report,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use mscent_world::Op;

    fn wait(job: &mut BuildJob) -> Result<Built, String> {
        loop {
            for u in job.poll() {
                if let Update::Finished(r) = u {
                    return r;
                }
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
    fn a_level_without_start_or_goal_builds_view_only() {
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
        });
        let mut job = start_cached(&doc, false, &SharedCache::default()).expect("starts");
        let built = wait(&mut job).expect("builds");
        assert!(built.unplayable().is_some());
    }

    #[test]
    fn the_showcase_builds_playable_with_progress() {
        let doc = Document::from_level(&mscent_world::showcase_level());
        let mut job = start_cached(&doc, true, &SharedCache::default()).expect("starts");
        let mut steps = Vec::new();
        let mut lines = Vec::new();
        let built = loop {
            let mut done = None;
            for u in job.poll() {
                match u {
                    Update::Step { step, text } => {
                        steps.push(step);
                        lines.push(text);
                    }
                    Update::Finished(r) => done = Some(r),
                }
            }
            if let Some(r) = done {
                break r.expect("builds");
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        };
        assert!(built.unplayable().is_none());
        assert!(
            steps.windows(2).all(|w| w[0] < w[1]) && steps.len() >= 4,
            "{steps:?}"
        );
        assert!(lines.iter().any(|l| l.starts_with("CSG:")), "{lines:?}");
        assert!(lines.last().unwrap().contains("Build finished"));
    }
}
