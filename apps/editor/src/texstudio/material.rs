//! Apply and Save: making a material on the worker and installing it, and writing
//! it first when it is a Save. A page is proved to make a texture before any file
//! is touched, because a row in `materials.txt` whose recipe fails stops the game
//! from loading any material at all. Save also installs, so the running table and
//! the disk give a new material the same id.

use mscent_render::{Material, Texture};
use mscent_texgen::Recipe;
use mscent_world::materials::NAMES;

use super::TexStudio;
use super::save;
use super::worker::{BakeJob, Baked};

/// What Save did with the request.
#[derive(Debug, PartialEq, Eq)]
pub enum SaveStart {
    Started,
    /// It would overwrite this stock material: ask, then [`TexStudio::consent`].
    NeedsConsent(String),
    Refused,
}

/// What Apply or Save asked the worker for.
pub(super) struct PendingBake {
    /// Write the files too, not only install the material.
    save: bool,
    /// Still to be put in the mailbox, which was busy.
    unposted: Option<BakeJob>,
}

impl TexStudio {
    pub(super) fn post_bake(&mut self) {
        let job = self.bake.as_mut().and_then(|b| b.unposted.take());
        let Some(job) = job else {
            return;
        };
        let refused = match self.worker() {
            Some(worker) => worker.post_bake(job).err(),
            None => {
                self.bake = None;
                return;
            }
        };
        if let (Some(job), Some(bake)) = (refused, self.bake.as_mut()) {
            bake.unposted = Some(*job);
        }
    }

    /// Whether a material is being made: Apply and Save go one at a time.
    pub fn is_baking(&self) -> bool {
        self.bake.is_some()
    }

    fn start_bake(&mut self, save: bool) -> bool {
        if self.bake.is_some() {
            self.message = Some("still making the last material".to_string());
            return false;
        }
        let Some(doc) = self.doc() else {
            self.message = Some("no material is open".to_string());
            return false;
        };
        let graph = match doc.page() {
            Ok(graph) => graph.clone(),
            Err(error) => {
                self.message = Some(format!("the page has an error: {}", error.message));
                return false;
            }
        };
        let job = BakeJob {
            name: doc.name().to_string(),
            recipe: doc.recipe().clone(),
            graph,
        };
        self.bake = Some(PendingBake {
            save,
            unposted: Some(job),
        });
        true
    }

    /// Makes the material and installs it in the running editor.
    pub fn apply(&mut self) -> bool {
        self.start_bake(false)
    }

    /// Makes the material, and if that works writes it and installs it, so the
    /// table and the disk give it the same id. Asks first about a stock material.
    pub fn save(&mut self) -> SaveStart {
        let Some(name) = self.current.clone() else {
            self.message = Some("no material is open".to_string());
            return SaveStart::Refused;
        };
        if NAMES.contains(&name.as_str()) && !self.overwrite_ok.contains(&name) {
            return SaveStart::NeedsConsent(name);
        }
        if self.store.dir().is_none() {
            self.message = Some("no texture directory to save to".to_string());
            return SaveStart::Refused;
        }
        if self.start_bake(true) {
            SaveStart::Started
        } else {
            SaveStart::Refused
        }
    }

    /// The answer to [`SaveStart::NeedsConsent`] was yes.
    pub fn consent(&mut self, name: &str) {
        self.overwrite_ok.insert(name.to_string());
    }

    pub(super) fn finish_bake(&mut self, baked: Baked) {
        let Some(pending) = self.bake.take() else {
            return;
        };
        let Baked {
            name,
            recipe,
            texture,
        } = baked;
        let said = match texture {
            Ok(texture) => self.finish_with(pending.save, &name, &recipe, texture),
            Err(why) => format!("{name} was not made: {why}"),
        };
        self.message = Some(said);
    }

    /// Writes and installs a finished material; what to say about it. On a failure
    /// the recipe stays as it is, still dirty, and the table is not touched.
    fn finish_with(&mut self, save: bool, name: &str, recipe: &Recipe, texture: Texture) -> String {
        let mut said = format!("Applied {name}");
        if save {
            let Some(dir) = self.store.dir() else {
                return "no texture directory to save to".to_string();
            };
            let known = self.table().and_then(|t| t.find(name)).map(|id| id.0);
            match save::save_material(&dir, name, recipe, known) {
                Ok(id) => said = format!("Saved {name} (id {id}) in {}", dir.display()),
                Err(why) => return format!("not saved: {why}"),
            }
            if let Some(doc) = self.docs.get_mut(name) {
                doc.mark_saved(recipe.clone());
            }
        }
        let Some(mut table) = self.table() else {
            return format!("{said}, but there is no table to show it in");
        };
        let material = Material {
            name: name.to_string(),
            gloss: recipe.gloss,
            texture,
        };
        if table.install(material).is_none() {
            return format!("{said}, but the material table is full");
        }
        self.store.replace(table);
        said
    }
}
