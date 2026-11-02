//! Opening a material of the table and making a new one. A recipe file is read and
//! parsed on the worker, since files can be large and a tick never waits; the
//! material opens on a later tick, and until then the one on show stays.

use std::collections::BTreeMap;
use std::io::Read;
use std::path::Path;

use mscent_render::MaterialTable;
use mscent_render::manifest::{DEFAULT_GLOSS, name_is_valid};
use mscent_render::recipe_texture::EXTENSION;
use mscent_texgen::recipe::{self, MAX_FILE};
use mscent_texgen::{Recipe, Size};

use super::TexStudio;
use super::worker::{OpenJob, Opened};
use crate::model::texdoc::TexDoc;

/// A recipe file being read.
pub(super) struct PendingOpen {
    name: String,
    /// What was on show when it was asked for: the material only takes the screen
    /// if the user has not gone elsewhere since.
    from: Option<String>,
    /// Still to be put in the mailbox, which was busy.
    unposted: Option<OpenJob>,
}

/// Reads and parses a recipe file, never holding more of it than the limit allows.
pub(super) fn read_recipe_file(path: &Path) -> Result<Recipe, String> {
    let describe = |e: &dyn std::fmt::Display| format!("cannot open {}: {e}", path.display());
    let file = std::fs::File::open(path).map_err(|e| describe(&e))?;
    let mut bytes = Vec::new();
    file.take(MAX_FILE as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| describe(&e))?;
    recipe::read(&bytes).map_err(|e| describe(&e))
}

impl TexStudio {
    /// Whether a recipe file is being read.
    #[cfg(test)]
    pub fn is_opening(&self) -> bool {
        self.opening.is_some()
    }

    pub(super) fn post_open(&mut self) {
        let job = self.opening.as_mut().and_then(|o| o.unposted.take());
        let Some(job) = job else {
            return;
        };
        let refused = match self.worker() {
            Some(worker) => worker.post_open(job).err(),
            None => {
                self.opening = None;
                return;
            }
        };
        if let (Some(job), Some(open)) = (refused, self.opening.as_mut()) {
            open.unposted = Some(*job);
        }
    }

    /// Opens a material of the table: where it was left if it is open, else its
    /// recipe, read on the worker. A material with no recipe is PNG files, so it
    /// starts on a new page of its size, and nothing replaces the files until it is
    /// saved. A recipe that cannot be read is a message, and the current material
    /// stays.
    pub fn open_material(&mut self, name: &str) {
        if self.docs.contains_key(name) {
            self.switch_to(name);
            return;
        }
        if let Some(open) = &self.opening {
            self.message = Some(format!("still opening {}", open.name));
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
        if let Some(path) = file {
            self.message = Some(format!("Opening {name}..."));
            self.opening = Some(PendingOpen {
                name: name.to_string(),
                from: self.current.clone(),
                unposted: Some(OpenJob {
                    name: name.to_string(),
                    path,
                    gloss: material.gloss,
                }),
            });
            return;
        }
        let texture = &material.texture;
        let Ok(size) = Size::new(texture.width(), texture.height()) else {
            self.message = Some(format!("{name} has a size recipes do not have"));
            return;
        };
        let gloss = material.gloss;
        self.message = Some(format!(
            "{name} has no recipe: this new page replaces its PNG files if saved"
        ));
        self.docs
            .insert(name.to_string(), TexDoc::first_page(name, size, gloss));
        self.switch_to(name);
    }

    /// The recipe arrived: the material is open, and on show unless the user went to
    /// another one while it was read.
    pub(super) fn finish_open(&mut self, opened: Opened) {
        let Some(pending) = self.opening.take() else {
            return;
        };
        match opened.recipe {
            Ok(recipe) => {
                let doc = TexDoc::open(&opened.name, recipe, opened.gloss);
                self.docs.insert(opened.name.clone(), doc);
                if self.current == pending.from {
                    self.switch_to(&opened.name);
                }
            }
            Err(why) => self.message = Some(why),
        }
    }

    /// Makes a new material, not yet in the table. Names follow the manifest's
    /// rules, and one already in use is refused: its saved id belongs to it.
    pub fn create(&mut self, name: &str, side: u32) -> Result<(), String> {
        let dir = self.store.dir();
        check_new_name(name, self.table().as_ref(), &self.docs, dir.as_deref())?;
        let size = Size::new(side, side).map_err(|e| e.to_string())?;
        self.docs
            .insert(name.to_string(), TexDoc::starter(name, size, DEFAULT_GLOSS));
        self.switch_to(name);
        Ok(())
    }
}

/// Why `name` cannot be a new material, if it cannot. A recipe file already in
/// the directory counts even when no row lists it: Save would replace it.
fn check_new_name(
    name: &str,
    table: Option<&MaterialTable>,
    docs: &BTreeMap<String, TexDoc>,
    dir: Option<&Path>,
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
    let file = dir.map(|d| d.join(format!("{name}{EXTENSION}")));
    if let Some(file) = file.filter(|f| f.try_exists().unwrap_or(true)) {
        return Err(format!(
            "{} already exists and is no material of this editor's: rename it first",
            file.display()
        ));
    }
    Ok(())
}
