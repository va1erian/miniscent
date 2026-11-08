//! The in-app guide: pages read from `assets/guide/scripting.md`, and a
//! reference page made from the API table. Every example in it is compiled by a
//! test, so the guide cannot teach what the language no longer accepts.
//!
//! The file is in the Markdown subset `pages.rs` reads, as the editor manual is.

use super::api::{API, Recv};
use crate::pages::parse;
pub use crate::pages::{Block, Page};

const TEXT: &str = include_str!("../../../../assets/guide/scripting.md");

const REFERENCE_ORDER: [Recv; 12] = [
    Recv::Global,
    Recv::Mover,
    Recv::Zone,
    Recv::Group,
    Recv::Enemy,
    Recv::Marker,
    Recv::Prop,
    Recv::Player,
    Recv::Kind,
    Recv::Array,
    Recv::Block,
    Recv::Value,
];

fn reference() -> Page {
    let mut blocks = vec![Block::Text(
        "Every message there is. The library's functions, such as `autoDoor:travel:key:`, are in assets/scripts/lib, and completion lists them too."
            .to_string(),
    )];
    for on in REFERENCE_ORDER {
        let mut rows = API.iter().filter(|e| e.on == on).peekable();
        if rows.peek().is_some() {
            blocks.push(Block::Heading(on.title().to_string()));
            blocks.push(Block::Head(
                "Message".to_string(),
                "What it does".to_string(),
            ));
        }
        blocks.extend(rows.map(|e| Block::Row(e.selector.to_string(), e.help.to_string())));
    }
    Page {
        title: "Reference".to_string(),
        blocks,
    }
}

/// Every page, in reading order.
pub fn pages() -> Vec<Page> {
    let mut all = parse(TEXT);
    all.push(reference());
    all
}

#[cfg(test)]
mod tests {
    use super::*;
    use mscent_game::{Game, natives};
    use mscent_script::{NameCheck, Source, Vm};

    #[test]
    fn the_guide_has_its_pages_and_each_has_a_title_and_something_in_it() {
        let pages = pages();
        assert!(pages.len() >= 7, "{}", pages.len());
        assert_eq!(pages[0].title, "Start here");
        assert_eq!(pages.last().unwrap().title, "Reference");
        for page in &pages {
            assert!(
                !page.title.is_empty() && !page.blocks.is_empty(),
                "{page:?}"
            );
        }
    }

    /// The library, from the assets: what a level sees when it runs.
    fn library() -> Vec<(String, String)> {
        miniscent_game::scripts::library_files(None)
    }

    #[test]
    fn every_example_in_the_guide_compiles() {
        let lib = library();
        let table = natives();
        let no_names: [NameCheck; 0] = [];
        for page in pages() {
            for block in &page.blocks {
                let Block::Code(code) = block else { continue };
                // Each with the library, as the script it would be pasted into.
                let mut sources: Vec<Source> = lib
                    .iter()
                    .map(|(name, text)| Source { name, text })
                    .collect();
                sources.push(Source {
                    name: "<game>",
                    text: "define waitArrived: m [ ] define waitDestroyed: g [ ]",
                });
                sources.push(Source {
                    name: "example",
                    text: code,
                });
                let loaded = Vm::<Game>::load(&sources, &table, &no_names);
                if let Err(errors) = loaded {
                    panic!("{}:\n{code}\n{errors:?}", page.title);
                }
            }
        }
    }
}
