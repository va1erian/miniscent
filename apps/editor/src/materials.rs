//! The material table of a running editor, replaceable while it runs. One table
//! serves every preview and the panels; a clone shares its pixels, and its mips
//! are built here, once, so a preview pays for neither. Each replacement is a
//! new generation, which is how the 3D view and the panels know to follow.

use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use miniscent_game::assets::load_materials_from;
use mscent_render::MaterialTable;

/// The store as shared between the UI thread and the preview jobs.
pub type SharedMaterials = Arc<MaterialStore>;

/// A table and its generation. The lock is held for moments, never across a
/// decode, so a UI tick can look at the generation.
#[derive(Default)]
pub struct MaterialStore {
    slot: Mutex<Slot>,
}

#[derive(Default)]
struct Slot {
    generation: u64,
    /// `None` until first asked for. A failed load is kept, as a table would
    /// be: the same directory would fail again on every preview.
    table: Option<Result<MaterialTable, String>>,
    /// Where the table came from, which is where Save writes.
    dir: Option<PathBuf>,
}

impl MaterialStore {
    fn slot(&self) -> MutexGuard<'_, Slot> {
        self.slot.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub fn generation(&self) -> u64 {
        self.slot().generation
    }

    /// The current table and its generation, loading it on the first call. Two
    /// first calls at once both decode (at 67 ms, rare and harmless), and
    /// a table installed meanwhile wins over both.
    pub fn current(&self) -> (u64, Result<MaterialTable, String>) {
        {
            let slot = self.slot();
            if let Some(table) = &slot.table {
                return (slot.generation, table.clone());
            }
        }
        let (loaded, dir) = match load_materials_from(None) {
            Ok((table, dir)) => (Ok(table.filtered()), Some(dir)),
            Err(why) => (Err(why), None),
        };
        let mut slot = self.slot();
        if slot.dir.is_none() {
            slot.dir = dir;
        }
        let table = slot.table.get_or_insert(loaded).clone();
        (slot.generation, table)
    }

    /// The directory the table came from, once it has been loaded.
    pub fn dir(&self) -> Option<PathBuf> {
        self.slot().dir.clone()
    }

    /// Points Save somewhere else (scripted runs and tests must not write to the
    /// source tree).
    pub fn set_dir(&self, dir: PathBuf) {
        self.slot().dir = Some(dir);
    }

    /// Installs `table` as a new generation and returns its number. Whoever
    /// watches the generation picks it up on its next look.
    pub fn replace(&self, table: MaterialTable) -> u64 {
        let table = table.filtered();
        let mut slot = self.slot();
        slot.table = Some(Ok(table));
        slot.generation = slot.generation.wrapping_add(1);
        slot.generation
    }
}

#[cfg(test)]
mod tests {
    use mscent_render::{Material, MaterialId, Texture};

    use super::*;

    fn one_material() -> MaterialTable {
        let mut table = MaterialTable::new();
        table.add(Material {
            name: "a".into(),
            gloss: 1,
            texture: Texture::solid(0xff00_0000),
        });
        table
    }

    #[test]
    fn the_first_look_loads_and_every_look_shares_the_pixels() {
        let store = MaterialStore::default();
        let (gen_a, a) = store.current();
        let (gen_b, b) = store.current();
        assert_eq!((gen_a, gen_b), (0, 0), "loading is no new generation");
        let (a, b) = (a.expect("stock"), b.expect("stock"));
        for i in 0..a.len() as u16 {
            let (x, y) = (a.get(MaterialId(i)), b.get(MaterialId(i)));
            let (x, y) = (x.expect("stock"), y.expect("stock"));
            assert!(std::ptr::eq(x, y), "material {i} was copied");
            assert!(x.texture.is_filtered(), "mips are made once, here");
        }
    }

    #[test]
    fn a_replacement_is_a_new_generation_and_filtered() {
        let store = MaterialStore::default();
        assert_eq!(store.replace(one_material()), 1);
        assert_eq!(store.generation(), 1);
        assert_eq!(store.replace(one_material()), 2);
        let (generation, table) = store.current();
        let table = table.expect("installed");
        assert_eq!((generation, table.len()), (2, 1));
        assert!(
            table
                .get(MaterialId(0))
                .expect("there")
                .texture
                .is_filtered()
        );
    }

    #[test]
    fn a_table_installed_first_is_not_overwritten_by_a_load() {
        let store = MaterialStore::default();
        store.replace(one_material());
        assert_eq!(store.current().1.expect("installed").len(), 1);
    }
}
