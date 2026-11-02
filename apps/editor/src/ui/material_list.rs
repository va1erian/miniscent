//! The brush panel's list of materials, and keeping the panels on the material
//! table of the current generation.

use mscent_render::MaterialTable;
use mscent_world::materials::NAMES;

use super::app::Editor;

/// The last row: an id the table has no material for (retired, or from a level
/// made with another manifest). No row may show a name that is not the brush's.
const OTHER: &str = "(other id)";

/// The rows of the brush panel's material list. A retired id has no row, so a
/// row number is not an id.
#[derive(Clone, Debug, PartialEq)]
pub struct MaterialChoices {
    rows: Vec<(u16, String)>,
}

impl Default for MaterialChoices {
    /// The stock names, for an editor with no textures to read: brushes can
    /// still be given the ids every manifest has.
    fn default() -> Self {
        let rows = (0u16..).zip(NAMES.map(String::from)).collect();
        Self { rows }
    }
}

impl MaterialChoices {
    pub fn from_table(table: &MaterialTable) -> Self {
        let rows = table.iter().map(|(id, m)| (id.0, m.name.clone())).collect();
        Self { rows }
    }

    /// The text of every row.
    pub fn labels(&self) -> Vec<&str> {
        let names = self.rows.iter().map(|(_, name)| name.as_str());
        names.chain([OTHER]).collect()
    }

    /// The row that shows material `id`.
    pub fn row_of(&self, id: u16) -> usize {
        let row = self.rows.iter().position(|&(i, _)| i == id);
        row.unwrap_or(self.rows.len())
    }

    /// The material a row picks; `None` for the last row, which picks nothing.
    pub fn id_of(&self, row: usize) -> Option<u16> {
        self.rows.get(row).map(|&(id, _)| id)
    }
}

impl Editor {
    /// A new generation of the material table reaches the panels here: their
    /// lists are remade and faces measure against the new texture sizes. Only the
    /// lists are refreshed, since a field may hold text being typed, and lists
    /// that could not be made are tried again on the next tick. The 3D view
    /// follows through the preview path by itself.
    pub(super) fn follow_materials(&mut self) {
        let store = self.studio.materials();
        if store.generation() == self.panel.materials_shown() {
            return;
        }
        let (generation, table) = store.current();
        match self.panel.show_materials(generation, table.ok().as_ref()) {
            Ok(sizes) => {
                self.state.borrow_mut().texture_sizes = sizes;
                self.panel.sync_material_lists(&self.state.borrow());
            }
            Err(e) => {
                if self.panel.note_failure(generation) {
                    self.push_log(&[format!("ERROR: material lists not updated: {e:?}")]);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use mscent_render::{Material, Texture};

    use super::*;

    fn material(name: &str) -> Material {
        Material {
            name: name.into(),
            gloss: 0,
            texture: Texture::solid(0),
        }
    }

    fn table() -> MaterialTable {
        let mut t = MaterialTable::new();
        for name in NAMES {
            t.add(material(name));
        }
        t.add(material("concrete"));
        t.add_retired();
        t.add(material("glass"));
        t
    }

    #[test]
    fn ids_past_the_stock_show_their_own_name() {
        let c = MaterialChoices::from_table(&table());
        let name = |id: u16| c.labels()[c.row_of(id)].to_string();
        assert_eq!(name(0), "brick");
        assert_eq!(name(4), "rock");
        assert_eq!(name(5), "concrete");
        assert_eq!(name(7), "glass");
    }

    #[test]
    fn a_retired_or_unknown_id_never_shows_another_materials_name() {
        let c = MaterialChoices::from_table(&table());
        assert_eq!(c.labels()[c.row_of(6)], OTHER);
        assert_eq!(c.labels()[c.row_of(8)], OTHER);
        assert_eq!(c.labels()[c.row_of(u16::MAX)], OTHER);
    }

    #[test]
    fn rows_pick_ids_across_a_retired_slot() {
        let c = MaterialChoices::from_table(&table());
        assert_eq!(c.id_of(5), Some(5));
        assert_eq!(c.id_of(6), Some(7), "row 6 is glass: id 6 has no row");
        assert_eq!(c.id_of(7), None, "the last row picks nothing");
        assert_eq!(c.id_of(usize::MAX), None);
    }

    #[test]
    fn with_no_textures_the_stock_names_remain() {
        let c = MaterialChoices::default();
        assert_eq!(c.labels(), [&NAMES[..], &[OTHER]].concat());
        assert_eq!(c.id_of(3), Some(3));
    }
}
