//! The editor manual: pages read from `assets/guide/editor.md`, in the Markdown
//! subset `pages.rs` reads. Help, Editor manual (F1) shows it in a window of
//! its own; F1 opens it on the page about the tool in hand. The scripting guide
//! (F7) is the script pane's, and the manual points there for the language.

use crate::model::{EntityKind, Tool};
use crate::pages::{Page, parse};

const TEXT: &str = include_str!("../../../assets/guide/editor.md");

/// Every page, in reading order.
pub fn pages() -> Vec<Page> {
    parse(TEXT)
}

/// The title of the page that is about `tool`.
fn title_for(tool: Tool) -> &'static str {
    match tool {
        Tool::Select | Tool::Box | Tool::Slice | Tool::Vertex => "Brush tools",
        Tool::Face => "Faces and textures",
        Tool::Voxel => "Voxels",
        Tool::Place(EntityKind::Light | EntityKind::Projector) => "Lighting",
        Tool::Place(EntityKind::VistaLayer) => "Vistas and mountains",
        Tool::Place(_) => "Entities",
        Tool::Kit(_) => "Scripts and movers",
    }
}

/// The page F1 opens on with `tool` in hand: the one about it, or the first.
pub fn topic_for(tool: Tool) -> usize {
    let title = title_for(tool);
    pages().iter().position(|p| p.title == title).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pages::Block;

    #[test]
    fn the_manual_has_its_pages_and_each_has_a_title_and_something_in_it() {
        let pages = pages();
        assert!(pages.len() >= 12, "{}", pages.len());
        assert_eq!(pages[0].title, "Start here");
        for page in &pages {
            assert!(
                !page.title.is_empty() && !page.blocks.is_empty(),
                "{page:?}"
            );
        }
        let mut titles: Vec<&str> = pages.iter().map(|p| p.title.as_str()).collect();
        titles.sort_unstable();
        titles.dedup();
        assert_eq!(titles.len(), pages.len(), "two pages share a title");
    }

    #[test]
    fn every_tool_opens_the_manual_on_a_page_about_it() {
        let pages = pages();
        let tools = [
            Tool::Select,
            Tool::Box,
            Tool::Face,
            Tool::Slice,
            Tool::Vertex,
            Tool::Voxel,
            Tool::Place(EntityKind::PlayerStart),
            Tool::Place(EntityKind::Light),
            Tool::Place(EntityKind::Projector),
            Tool::Place(EntityKind::Enemy),
            Tool::Place(EntityKind::Goal),
            Tool::Place(EntityKind::Marker),
            Tool::Place(EntityKind::Zone),
            Tool::Place(EntityKind::VistaLayer),
            Tool::Kit(crate::script::kit::KitKind::Door),
        ];
        for tool in tools {
            let title = title_for(tool);
            assert!(
                pages.iter().any(|p| p.title == title),
                "no page {title:?} for {tool:?}"
            );
        }
    }

    #[test]
    fn no_page_has_a_paragraph_of_table_pipes_or_an_empty_row() {
        for page in pages() {
            for block in &page.blocks {
                match block {
                    Block::Text(t) => assert!(!t.starts_with('|'), "{}: {t}", page.title),
                    Block::Row(a, b) => {
                        assert!(!a.is_empty() && !b.is_empty(), "{}", page.title);
                    }
                    _ => {}
                }
            }
        }
    }
}
