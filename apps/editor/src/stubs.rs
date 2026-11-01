//! File commands the next work item implements. Each returns the status-bar
//! message shown when the menu entry is chosen.

use crate::model::Document;

pub fn open() -> String {
    "Open: not implemented yet".into()
}

pub fn save(doc: &Document) -> String {
    format!(
        "Save: not implemented yet ({} brushes, {} entities)",
        doc.brushes().len(),
        doc.entities().len()
    )
}

/// Checks that every brush is valid; the real build comes later.
pub fn build(doc: &Document) -> String {
    match doc.to_level() {
        Ok(level) => format!(
            "Build: not implemented yet ({} brushes are valid)",
            level.brushes.len()
        ),
        Err(e) => format!("Build: a brush is invalid ({e:?})"),
    }
}

pub fn play(doc: &Document) -> String {
    format!(
        "Play: not implemented yet{}",
        if doc.brushes().is_empty() {
            " (empty level)"
        } else {
            ""
        }
    )
}
