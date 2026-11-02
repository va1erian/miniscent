//! Editing a level's script in the editor: the buffer, checking it as it is
//! typed, what completion offers, basic entities, samples and the guide. No UI
//! types here, as in the model; `ui/script_pane.rs` is the widgets.

pub mod api;
pub mod area;
pub mod buffer;
pub mod check;
pub mod complete;
pub mod guide;
pub mod kit;
pub mod samples;
pub mod scan;

use crate::model::EditorState;
use crate::model::document::Document;
use complete::LevelNames;
use mscent_world::Entity;

fn sorted(mut names: Vec<String>) -> Vec<String> {
    names.sort();
    names.dedup();
    names
}

/// The names a script can use in `doc`.
pub fn level_names(doc: &Document) -> LevelNames {
    let (mut zones, mut markers, mut enemies) = (Vec::new(), Vec::new(), Vec::new());
    let mut tags: Vec<String> = doc.groups().iter().flat_map(|g| g.tags.clone()).collect();
    for def in doc.entities() {
        let Some(label) = doc.label(def.id) else {
            continue;
        };
        tags.extend(label.tags.iter().cloned());
        let list = match def.entity {
            Entity::Zone { .. } => &mut zones,
            Entity::Marker { .. } => &mut markers,
            Entity::Enemy { .. } => &mut enemies,
            _ => continue,
        };
        list.extend(label.name.clone());
    }
    LevelNames {
        movers: sorted(doc.groups().iter().map(|g| g.name.clone()).collect()),
        zones: sorted(zones),
        markers: sorted(markers),
        enemies: sorted(enemies),
        tags: sorted(tags),
    }
}

impl EditorState {
    /// Reads the script beside the level again, if it is time.
    pub fn poll_script(&mut self) {
        let path = self.doc.level_path().map(|p| p.with_extension("st"));
        self.script.poll(path.as_deref());
    }
}
