//! The pages as Markdown, read back by the view's own parser: what a page
//! shows, its examples and links, and the highlighter.

use super::*;
use crate::script::guide;
use Examples::{Plain, Script};
use xui_markdown::{BlockKind, Document, Inline};

fn page(blocks: Vec<Block>) -> Page {
    Page {
        title: "T & <U>".into(),
        blocks,
    }
}

/// The text a paragraph of `inline(text)` shows, and its inline styles.
fn shown(text: &str) -> (String, Vec<Inline>) {
    let doc = Document::parse(&inline(text));
    let block = &doc.blocks()[0];
    assert_eq!(block.kind, BlockKind::Paragraph, "{text:?}");
    let styles = doc.spans(block).iter().map(|s| s.style).collect();
    (doc.block_text(block).to_owned(), styles)
}

/// Every link destination in `doc`.
fn links(doc: &Document) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for block in doc.blocks() {
        for span in doc.spans(block) {
            if let Some(link) = span.link {
                let href = doc.link(link).to_owned();
                if out.last() != Some(&href) {
                    out.push(href);
                }
            }
        }
    }
    out
}

#[test]
fn text_shows_as_written_and_only_matched_markers_make_code_and_bold() {
    for text in [
        "a < b & c",
        "<script>alert(1)</script>",
        "2 * 3 `open",
        "**",
        "a_b_c and *one* [x](y) ~~z~~ | # !",
        "- not a list",
        "3. not a number",
        "> not a quote",
        "&amp; stays",
    ] {
        assert_eq!(shown(text).0, text, "{text:?}");
    }
    let (text, styles) = shown("use `x < 1` and **now**");
    assert_eq!(text, "use x < 1 and now");
    assert_eq!(
        styles,
        [Inline::NONE, Inline::CODE, Inline::NONE, Inline::BOLD]
    );
}

#[test]
fn an_example_is_a_script_block_offered_for_insertion() {
    let made = render(
        &page(vec![
            Block::Code("x := 1 < 2.".into()),
            Block::Code("say: 'hi'.\n```nested```".into()),
        ]),
        Script,
    );
    assert_eq!(
        made.examples,
        ["x := 1 < 2.\n", "say: 'hi'.\n```nested```\n"]
    );
    let doc = Document::parse(&made.markdown);
    let codes: Vec<_> = doc
        .blocks()
        .iter()
        .filter(|b| matches!(b.kind, BlockKind::Code { .. }))
        .collect();
    assert_eq!(codes.len(), 2, "{}", made.markdown);
    assert_eq!(doc.block_text(codes[0]), "x := 1 < 2.");
    assert_eq!(doc.block_text(codes[1]), "say: 'hi'.\n```nested```");
    assert!(matches!(&codes[0].kind, BlockKind::Code { lang } if &**lang == "script"));
    assert_eq!(links(&doc), ["insert:0", "insert:1"]);
    assert_eq!(doc.block_text(&doc.blocks()[0]), "T & <U>");
}

#[test]
fn a_plain_example_is_shown_as_written_and_offers_nothing() {
    let made = render(
        &page(vec![Block::Code("Ctrl+<wheel> & drag".into())]),
        Plain,
    );
    assert!(made.examples.is_empty());
    let doc = Document::parse(&made.markdown);
    assert!(links(&doc).is_empty(), "{}", made.markdown);
    let code = &doc.blocks()[1];
    assert_eq!(code.kind, BlockKind::Code { lang: "".into() });
    assert_eq!(doc.block_text(code), "Ctrl+<wheel> & drag");
}

#[test]
fn only_our_own_links_name_an_example() {
    assert_eq!(example_of("insert:3"), Some(3));
    for other in [
        "insert:",
        "insert:-1",
        "insert:x",
        "https://example.com",
        "file:///c:/x",
        "",
    ] {
        assert_eq!(example_of(other), None, "{other}");
    }
}

#[test]
fn rows_are_one_table_with_names_as_code() {
    let made = render(
        &page(vec![
            Block::Heading("Zone".into()),
            Block::Head("Message".into(), "What it does".into()),
            Block::Row("a:".into(), "one".into()),
            Block::Row("b:|c".into(), "two | three".into()),
            Block::Text("After.".into()),
        ]),
        Script,
    );
    let doc = Document::parse(&made.markdown);
    let rows: Vec<_> = doc
        .blocks()
        .iter()
        .filter(|b| matches!(b.kind, BlockKind::TableRow { .. }))
        .collect();
    assert_eq!(rows.len(), 3, "{}", made.markdown);
    let cells = |b| -> Vec<String> {
        doc.cells(b)
            .iter()
            .map(|c| doc.text()[c.start as usize..c.end as usize].to_owned())
            .collect()
    };
    assert_eq!(cells(rows[0]), ["Message", "What it does"]);
    assert_eq!(cells(rows[2]), ["b:|c", "two | three"]);
    assert_eq!(doc.spans(rows[1])[0].style, Inline::CODE);
    assert_eq!(doc.block_text(doc.blocks().last().unwrap()), "After.");
}

#[test]
fn rows_with_no_head_get_an_empty_one() {
    let made = render(&page(vec![Block::Row("a:".into(), "one".into())]), Script);
    let doc = Document::parse(&made.markdown);
    let rows = doc
        .blocks()
        .iter()
        .filter(|b| matches!(b.kind, BlockKind::TableRow { .. }))
        .count();
    assert_eq!(rows, 2, "{}", made.markdown);
}

#[test]
fn every_guide_page_has_a_link_for_each_example() {
    for page in guide::pages() {
        let made = render(&page, Script);
        let codes = page
            .blocks
            .iter()
            .filter(|b| matches!(b, Block::Code(_)))
            .count();
        assert_eq!(made.examples.len(), codes, "{}", page.title);
        let doc = Document::parse(&made.markdown);
        let expected: Vec<String> = (0..codes).map(|i| format!("insert:{i}")).collect();
        assert_eq!(links(&doc), expected, "{}", page.title);
    }
}

#[test]
fn every_manual_page_shows_its_examples_and_no_links() {
    for page in crate::manual::pages() {
        let made = render(&page, Plain);
        let doc = Document::parse(&made.markdown);
        let codes = page
            .blocks
            .iter()
            .filter(|b| matches!(b, Block::Code(_)))
            .count();
        let shown = doc
            .blocks()
            .iter()
            .filter(|b| matches!(b.kind, BlockKind::Code { .. }))
            .count();
        assert_eq!(shown, codes, "{}", page.title);
        assert!(links(&doc).is_empty(), "{}", page.title);
    }
}

#[test]
fn the_reference_lists_every_message() {
    let pages = guide::pages();
    let made = render(pages.last().expect("a reference page"), Script);
    let doc = Document::parse(&made.markdown);
    for entry in crate::script::api::API {
        assert!(doc.text().contains(entry.selector), "{}", entry.selector);
    }
}

#[test]
fn the_highlighter_colours_script_in_bytes_and_nothing_else() {
    let line = "say: 'héllo' 12.";
    let (spans, _) = ScriptHighlight.line(SCRIPT, line, 0);
    let classed: Vec<(&str, CodeClass)> = spans
        .iter()
        .map(|s| (&line[s.range.clone()], s.class))
        .collect();
    assert!(
        classed.contains(&("say:", CodeClass::Function)),
        "{classed:?}"
    );
    assert!(
        classed.contains(&("'héllo'", CodeClass::String)),
        "{classed:?}"
    );
    assert!(
        spans
            .iter()
            .all(|s| line.is_char_boundary(s.range.start) && line.is_char_boundary(s.range.end))
    );
    assert!(ScriptHighlight.line("", line, 0).0.is_empty());
}

#[test]
fn the_highlighter_carries_an_open_string_to_the_next_line() {
    let (_, state) = ScriptHighlight.line(SCRIPT, "say: 'open", 0);
    assert_ne!(state, 0);
    let (spans, _) = ScriptHighlight.line(SCRIPT, "still' x", state);
    assert_eq!(spans[0].class, CodeClass::String);
    assert_eq!(spans[0].range, 0..6);
}

#[test]
fn the_palette_follows_the_theme_accent() {
    let theme = Theme::dark();
    let palette = palette(&theme);
    assert_eq!(palette.dark.function, theme.accent);
    assert_ne!(palette.light.keyword, palette.dark.keyword);
}
