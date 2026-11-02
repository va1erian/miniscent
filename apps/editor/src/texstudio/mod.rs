//! Everything between the recipe being edited and the workspace that needs no
//! widgets: which recipes are open, what the worker was last asked for, what it
//! answered, and Apply and Save. The xui shell calls [`TexStudio::tick`] on a timer
//! and it never waits; the whole loop is testable headless. Edits made here touch
//! no [`crate::model::Document`], so they start no level work.
//!
//! Every material opened in a session stays open, so moving between them loses no
//! edits, and a failed Save loses none either.

use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;
use std::time::{Duration, Instant};

use mscent_render::MaterialTable;
use mscent_render::manifest::{DEFAULT_GLOSS, name_is_valid};
use mscent_render::recipe_texture::EXTENSION;
use mscent_texgen::recipe::{self, MAX_FILE};
use mscent_texgen::{Recipe, Size};
use mscent_world::materials::{LIGHTPANEL, NAMES};

use crate::materials::SharedMaterials;
use crate::model::texdoc::TexDoc;

mod eval;
mod lit;
mod material;
mod paint;
mod save;
mod worker;

use eval::Request;
pub use eval::{Failure, Picture, Tab, Thumbs};
use material::PendingBake;
pub use material::SaveStart;
pub use paint::{Brush, Stroking};
pub(crate) use save::{updated_manifest, write_atomic};
use worker::Worker;

/// After this long with no drag edit the picture goes to full size: a slider can
/// end its drag without saying so, when the pointer leaves it.
const DRAG_PAUSE: Duration = Duration::from_millis(250);

/// A row of the material list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    pub name: String,
    pub dirty: bool,
    pub current: bool,
}

/// What one [`TexStudio::tick`] brought.
#[derive(Default)]
pub struct TexTick {
    pub picture: Option<Picture>,
    pub thumbs: Option<Thumbs>,
    /// Text for the status bar.
    pub message: Option<String>,
    /// Another material is showing: the old picture and thumbnails must go.
    pub cleared: bool,
}

/// What the worker was last asked for, to know when to ask again.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Asked {
    name: String,
    revision: u64,
    tab: Tab,
    draft_only: bool,
}

pub struct TexStudio {
    store: SharedMaterials,
    docs: BTreeMap<String, TexDoc>,
    current: Option<String>,
    tab: Tab,
    tiled: bool,
    open: bool,
    worker: Option<Worker>,
    seq: u64,
    asked: Option<Asked>,
    /// The sequence number the last request went out with.
    asked_seq: u64,
    /// The last request of the material before this one, whose answers are old.
    floor: u64,
    /// The request the picture on show answers, and whether it was the full one.
    shown: Option<(u64, bool)>,
    failure: Option<Failure>,
    /// When a parameter was last dragged.
    dragged_at: Option<Instant>,
    bake: Option<PendingBake>,
    /// Stock materials whose overwriting was agreed to.
    overwrite_ok: BTreeSet<String>,
    /// The worker has yet to hear that the workspace went away.
    idle_unsent: bool,
    /// Waiting to go to the status bar.
    message: Option<String>,
    cleared: bool,
    /// The paint tool: the workspace's, so none of it is in a recipe or its undo.
    tool: paint::Tool,
}

fn read_recipe_file(path: &std::path::Path) -> Result<Recipe, String> {
    let describe = |e: &dyn std::fmt::Display| format!("cannot open {}: {e}", path.display());
    let file = std::fs::File::open(path).map_err(|e| describe(&e))?;
    let mut bytes = Vec::new();
    file.take(MAX_FILE as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| describe(&e))?;
    recipe::read(&bytes).map_err(|e| describe(&e))
}

impl TexStudio {
    pub fn new(store: SharedMaterials) -> Self {
        Self {
            store,
            docs: BTreeMap::new(),
            current: None,
            tab: Tab::Block,
            tiled: false,
            open: false,
            worker: None,
            seq: 0,
            asked: None,
            asked_seq: 0,
            floor: 0,
            shown: None,
            failure: None,
            dragged_at: None,
            bake: None,
            overwrite_ok: BTreeSet::new(),
            idle_unsent: false,
            message: None,
            cleared: false,
            tool: paint::Tool::default(),
        }
    }

    /// The workspace is coming up or going away. Nothing runs while it is away,
    /// but a Save in progress finishes.
    pub fn set_open(&mut self, open: bool) {
        self.open = open;
        self.asked = None;
        self.idle_unsent = !open;
    }

    /// Whether anything is left for ticks to do. While nothing is, the workspace
    /// costs the UI tick nothing.
    pub fn needs_ticks(&self) -> bool {
        self.open || self.idle_unsent || self.bake.is_some()
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn doc(&self) -> Option<&TexDoc> {
        self.docs.get(self.current.as_deref()?)
    }

    fn doc_mut(&mut self) -> Option<&mut TexDoc> {
        self.docs.get_mut(self.current.as_deref()?)
    }

    pub fn tab(&self) -> Tab {
        self.tab
    }

    pub fn set_tab(&mut self, tab: Tab) {
        self.tab = tab;
    }

    /// Puts a message on its way to the status bar.
    pub fn say(&mut self, message: String) {
        self.message = Some(message);
    }

    pub fn tiled(&self) -> bool {
        self.tiled
    }

    pub fn set_tiled(&mut self, tiled: bool) {
        self.tiled = tiled;
    }

    /// The blocks to draw as broken, and what to say about them.
    pub fn trouble(&self) -> (Vec<usize>, Option<String>) {
        let page = self.doc().map(TexDoc::page);
        if let Some(Err(error)) = page {
            return (error.blocks.clone(), Some(error.message.clone()));
        }
        match &self.failure {
            Some(f) => (f.block.into_iter().collect(), Some(f.why.clone())),
            None => (Vec::new(), None),
        }
    }

    /// The table on show, if there is one to read.
    fn table(&self) -> Option<MaterialTable> {
        self.store.current().1.ok()
    }

    /// The list: the table's materials by id, then new ones not applied yet.
    pub fn rows(&self) -> Vec<Row> {
        let table = self.table();
        let in_table: Vec<String> = table
            .iter()
            .flat_map(|t| t.iter().map(|(_, m)| m.name.clone()))
            .collect();
        let extra = self.docs.keys().filter(|n| !in_table.contains(n)).cloned();
        in_table
            .iter()
            .cloned()
            .chain(extra.collect::<Vec<_>>())
            .map(|name| Row {
                dirty: self.docs.get(&name).is_some_and(TexDoc::dirty),
                current: self.current.as_deref() == Some(name.as_str()),
                name,
            })
            .collect()
    }

    fn switch_to(&mut self, name: &str) {
        self.current = Some(name.to_string());
        self.floor = self.seq;
        self.asked = None;
        self.shown = None;
        self.failure = None;
        self.dragged_at = None;
        self.cleared = true;
    }

    /// Opens a material of the table: where it was left if it is open, else its
    /// recipe. A material with no recipe is PNG files, so it starts on a new page
    /// of its size, and nothing replaces the files until it is saved. A recipe that
    /// cannot be read is a message, and the current material stays.
    pub fn open_material(&mut self, name: &str) {
        if self.docs.contains_key(name) {
            self.switch_to(name);
            return;
        }
        let Some(table) = self.table() else {
            self.message = Some("no material table to open from".to_string());
            return;
        };
        let material = table.find(name).and_then(|id| table.get(id));
        let Some(material) = material else {
            self.message = Some(format!("there is no material {name}"));
            return;
        };
        let file = self
            .store
            .dir()
            .map(|d| d.join(format!("{name}{EXTENSION}")))
            .filter(|p| p.is_file());
        let doc = match file {
            Some(path) => match read_recipe_file(&path) {
                Ok(recipe) => TexDoc::open(name, recipe, material.gloss),
                Err(why) => {
                    self.message = Some(why);
                    return;
                }
            },
            None => {
                let texture = &material.texture;
                let Ok(size) = Size::new(texture.width(), texture.height()) else {
                    self.message = Some(format!("{name} has a size recipes do not have"));
                    return;
                };
                self.message = Some(format!(
                    "{name} has no recipe: this new page replaces its PNG files if saved"
                ));
                TexDoc::first_page(name, size, material.gloss)
            }
        };
        self.docs.insert(name.to_string(), doc);
        self.switch_to(name);
    }

    /// Makes a new material, not yet in the table. Names follow the manifest's
    /// rules, and one already in use is refused: its saved id belongs to it.
    pub fn create(&mut self, name: &str, side: u32) -> Result<(), String> {
        check_new_name(name, self.table().as_ref(), &self.docs)?;
        let size = Size::new(side, side).map_err(|e| e.to_string())?;
        self.docs
            .insert(name.to_string(), TexDoc::starter(name, size, DEFAULT_GLOSS));
        self.switch_to(name);
        Ok(())
    }

    /// Runs an edit of the current recipe; an error goes to the status bar.
    pub fn edit<T>(&mut self, f: impl FnOnce(&mut TexDoc) -> Result<T, String>) -> Option<T> {
        let doc = self.doc_mut()?;
        match f(doc) {
            Ok(value) => Some(value),
            Err(why) => {
                self.message = Some(why);
                None
            }
        }
    }

    /// Notes a `live` edit: the picture stays a draft while they keep coming.
    pub fn note_drag(&mut self, live: bool) {
        self.dragged_at = live.then(Instant::now);
    }

    pub fn undo(&mut self) -> bool {
        self.dragged_at = None;
        self.doc_mut().is_some_and(TexDoc::undo)
    }

    pub fn redo(&mut self) -> bool {
        self.dragged_at = None;
        self.doc_mut().is_some_and(TexDoc::redo)
    }

    fn is_dragging(&mut self) -> bool {
        let dragging = self.dragged_at.is_some_and(|at| at.elapsed() < DRAG_PAUSE);
        if !dragging {
            self.dragged_at = None;
        }
        dragging
    }

    /// Whether the picture has caught up with the recipe as far as it will: the
    /// newest request is answered, in full, or could not be, or there is nothing
    /// to ask. Scripted runs wait for this.
    pub fn settled(&self) -> bool {
        let Some(doc) = self.doc() else {
            return true;
        };
        let asked_for_this = self.asked.as_ref().is_some_and(|a| {
            a.name == doc.name()
                && a.revision == doc.revision()
                && a.tab == self.tab
                && !a.draft_only
        });
        if !asked_for_this {
            return false;
        }
        let answered = self.shown == Some((self.asked_seq, true));
        let failed = self
            .failure
            .as_ref()
            .is_some_and(|f| f.seq == self.asked_seq);
        doc.page().is_err() || answered || failed
    }

    fn worker(&mut self) -> Option<&Worker> {
        if self.worker.as_ref().is_some_and(Worker::died) {
            self.worker = None;
            self.asked = None;
            // Whatever it was making is gone, and nothing would ever say so.
            self.bake = None;
            self.message = Some("the texture thread stopped; starting another".to_string());
        }
        if self.worker.is_none() {
            match Worker::spawn() {
                Ok(worker) => self.worker = Some(worker),
                Err(why) => self.message = Some(why),
            }
        }
        self.worker.as_ref()
    }

    /// Asks the worker for the page as it is now, if that is not what it was last
    /// asked for. A page that does not compile asks for nothing: the last good
    /// picture stays.
    fn ask(&mut self) {
        let draft_only = self.is_dragging();
        let Some(doc) = self.doc() else {
            return;
        };
        let now = Asked {
            name: doc.name().to_string(),
            revision: doc.revision(),
            tab: self.tab,
            draft_only,
        };
        if self.asked.as_ref() == Some(&now) {
            return;
        }
        let Ok(graph) = doc.page() else {
            self.asked = Some(now);
            return;
        };
        let request = Request {
            seq: self.seq + 1,
            recipe: doc.recipe().clone(),
            graph: graph.clone(),
            selected: doc.selected(),
            tab: self.tab,
            draft_only,
            emissive: doc.name() == NAMES[usize::from(LIGHTPANEL)],
        };
        let Some(worker) = self.worker() else {
            return;
        };
        // Busy this moment: nothing is noted, so the next tick asks again.
        if worker.post(request).is_ok() {
            self.seq += 1;
            self.asked_seq = self.seq;
            self.asked = Some(now);
        }
    }

    /// Takes what the worker finished, keeping the newest of what can be shown.
    fn collect(&mut self, tick: &mut TexTick) {
        let Some(outbox) = self.worker.as_ref().and_then(Worker::take) else {
            return;
        };
        // Nothing older than what is on show, and nothing of the last material.
        let oldest = self.shown.map_or(self.floor + 1, |s| s.0);
        tick.picture = outbox
            .picture
            .filter(|p| p.seq >= oldest && p.tab == self.tab);
        tick.thumbs = outbox.thumbs.filter(|t| t.seq >= oldest);
        if let Some(p) = &tick.picture {
            self.shown = Some((p.seq, p.full));
            if self.failure.as_ref().is_some_and(|f| f.seq < p.seq) {
                self.failure = None;
            }
        }
        if let Some(failure) = outbox.failure
            && failure.seq >= oldest
        {
            self.failure = Some(failure);
        }
        if let Some(baked) = outbox.baked {
            self.finish_bake(baked);
        }
    }

    /// One UI tick. Never waits: when a lock is busy the work is left for the next.
    pub fn tick(&mut self) -> TexTick {
        let mut tick = TexTick {
            cleared: std::mem::take(&mut self.cleared),
            ..TexTick::default()
        };
        if self.open || self.bake.is_some() {
            self.worker();
        }
        self.post_bake();
        if self.idle_unsent {
            // Busy this moment, or never started: left for the next tick.
            self.idle_unsent = self.worker.as_ref().is_some_and(|w| !w.post_idle());
        }
        if self.open {
            self.ask();
        }
        if self.open || self.bake.is_some() {
            self.collect(&mut tick);
        }
        tick.message = self.message.take();
        tick
    }
}

/// Why `name` cannot be a new material, if it cannot.
fn check_new_name(
    name: &str,
    table: Option<&MaterialTable>,
    docs: &BTreeMap<String, TexDoc>,
) -> Result<(), String> {
    if !name_is_valid(name) {
        return Err(format!(
            "{name:?}: names are 1 to 48 of a-z, 0-9 and -, with a letter or digit at each \
             end, and no Windows device name"
        ));
    }
    if docs.contains_key(name) || table.is_some_and(|t| t.find(name).is_some()) {
        return Err(format!("there is already a material {name}"));
    }
    Ok(())
}

#[cfg(test)]
mod hostile_tests;
#[cfg(test)]
mod paint_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod tick_tests;
