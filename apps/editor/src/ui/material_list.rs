//! The brush panel's list of materials, and keeping the panels on the material
//! table of the current generation.

use mscent_render::MaterialTable;
use mscent_world::materials::{NAMES, VIEW_SLOTS, view_material};

use super::app::Editor;

/// The last row: an id the table has no material for (retired, or from a level
/// made with another manifest). No row may show a name that is not the brush's.
const OTHER: &str = "(other id)";

/// The viewport materials, after the textures: "View 0" to "View 7". Their ids
/// are far above any texture's, so a row number is never one of them.
pub fn view_rows() -> impl Iterator<Item = (u16, String)> {
    (0..VIEW_SLOTS).filter_map(|slot| Some((view_material(slot)?, format!("View {slot}"))))
}

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
        let stock = (0u16..).zip(NAMES.map(String::from));
        let rows = stock.chain(view_rows()).collect();
        Self { rows }
    }
}

impl MaterialChoices {
    pub fn from_table(table: &MaterialTable) -> Self {
        let textures = table.iter().map(|(id, m)| (id.0, m.name.clone()));
        let rows = textures.chain(view_rows()).collect();
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
        assert_eq!(
            c.id_of(7),
            view_material(0),
            "the views follow the textures"
        );
        assert_eq!(
            c.id_of(7 + usize::from(VIEW_SLOTS)),
            None,
            "the last row picks nothing"
        );
        assert_eq!(c.id_of(usize::MAX), None);
    }

    #[test]
    fn with_no_textures_the_stock_names_remain() {
        let c = MaterialChoices::default();
        let views = ["View 0", "View 1", "View 2", "View 3"];
        let views = [&views[..], &["View 4", "View 5", "View 6", "View 7"]].concat();
        assert_eq!(c.labels(), [&NAMES[..], &views, &[OTHER]].concat());
        assert_eq!(c.id_of(3), Some(3));
    }

    #[test]
    fn a_view_material_shows_its_own_row_and_picks_its_own_id() {
        for choices in [
            MaterialChoices::from_table(&table()),
            MaterialChoices::default(),
        ] {
            for slot in 0..VIEW_SLOTS {
                let id = view_material(slot).expect("a slot");
                let row = choices.row_of(id);
                assert_eq!(choices.labels()[row], format!("View {slot}"));
                assert_eq!(choices.id_of(row), Some(id), "the mapping holds both ways");
            }
            // The id just above the slots is not one of them.
            let none = choices.row_of(mscent_world::materials::NO_DRAW);
            assert_eq!(choices.labels()[none], OTHER);
        }
    }
}
