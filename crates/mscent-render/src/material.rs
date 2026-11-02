//! Materials and the table that gives them ids. `materials.txt` decides the ids
//! ([`crate::manifest`]), and clones of a table share the materials' pixels, so
//! an editor can hand one to every preview for the price of the pointers.

use std::path::Path;
use std::sync::Arc;

use crate::manifest::{self, Entry};
use crate::texture::{Texture, TextureError, load_texture};

/// Index of a material in a [`MaterialTable`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct MaterialId(pub u16);

/// A named surface: its textures and how shiny it is.
#[derive(Clone, Debug)]
pub struct Material {
    pub name: String,
    /// Specular power input, 0 (matte) to 255 (mirror-like).
    pub gloss: u8,
    pub texture: Texture,
}

/// All materials of a level, addressed by [`MaterialId`]. A retired id keeps its
/// slot, empty, so the ids after it stay where levels expect them.
#[derive(Clone, Debug, Default)]
pub struct MaterialTable {
    materials: Vec<Option<Arc<Material>>>,
}

impl MaterialTable {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a material; `None` when the table is full (65536 entries).
    pub fn add(&mut self, material: Material) -> Option<MaterialId> {
        let id = self.next_id()?;
        self.materials.push(Some(Arc::new(material)));
        Some(id)
    }

    /// Reserve the next id for nothing; `None` when the table is full. Faces
    /// that still use it draw the scene's grey fallback.
    pub fn add_retired(&mut self) -> Option<MaterialId> {
        let id = self.next_id()?;
        self.materials.push(None);
        Some(id)
    }

    fn next_id(&self) -> Option<MaterialId> {
        u16::try_from(self.materials.len()).ok().map(MaterialId)
    }

    /// `None` for an id that was retired or never existed.
    pub fn get(&self, id: MaterialId) -> Option<&Material> {
        self.materials.get(usize::from(id.0))?.as_deref()
    }

    pub fn find(&self, name: &str) -> Option<MaterialId> {
        self.iter().find(|(_, m)| m.name == name).map(|(id, _)| id)
    }

    /// The materials that exist, by id, skipping retired ones.
    pub fn iter(&self) -> impl Iterator<Item = (MaterialId, &Material)> {
        (0u16..)
            .zip(&self.materials)
            .filter_map(|(id, m)| Some((MaterialId(id), m.as_deref()?)))
    }

    /// The same materials with smooth sampling on (see
    /// [`Texture::filtered`]). Costs memory for the mip chains and some
    /// time per pixel, which is why it is a choice and not the default. What is
    /// filtered already stays shared; anything else that is shared is copied
    /// first, so other holders of the table do not see the change.
    pub fn filtered(mut self) -> Self {
        for m in self.materials.iter_mut().flatten() {
            if !m.texture.is_filtered() {
                let m = Arc::make_mut(m);
                m.texture = std::mem::replace(&mut m.texture, Texture::solid(0)).filtered();
            }
        }
        self
    }

    /// Number of ids, retired ones included.
    pub fn len(&self) -> usize {
        self.materials.len()
    }

    pub fn is_empty(&self) -> bool {
        self.materials.is_empty()
    }

    /// Load the materials `dir/materials.txt` lists, with the ids it gives them.
    /// PNGs it does not list are ignored, and a listed one that is missing is an
    /// error: the manifest is the whole truth, so a stray file cannot shift an
    /// id and a missing file cannot pass unnoticed. A directory with no manifest
    /// is read as [`manifest::from_listing`] says.
    pub fn load_dir(dir: &Path) -> Result<Self, TextureError> {
        let entries = match manifest::read(&dir.join(manifest::FILE))? {
            Some(entries) => entries,
            None => manifest::from_listing(dir)?,
        };
        let mut table = Self::new();
        for entry in entries {
            let added = match entry {
                Entry::Retired => table.add_retired(),
                Entry::Material { name, gloss } => {
                    let texture = load_texture(dir, &name)?;
                    table.add(Material {
                        name,
                        gloss,
                        texture,
                    })
                }
            };
            added.ok_or(TextureError::BadSize)?;
        }
        Ok(table)
    }
}

#[cfg(test)]
#[path = "material_tests.rs"]
mod material_tests;
