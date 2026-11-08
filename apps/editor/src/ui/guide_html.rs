//! A page of the scripting guide or of the editor manual as one HTML document,
//! for `xui-blitz` to lay out.
//!
//! The page is made here and shown by a single view, so it costs the window's
//! re-flow nothing, where the same words as separate widgets cost 100 ms. All
//! the HTML is made from the guide's own text, escaped, with the theme's colours
//! in a stylesheet. The only links are the ones made here, `insert:N` for the
//! page's Nth example, and `example_of` is the only thing that reads one back:
//! anything else a click reports is ignored, so the page can reach nothing.

use xui::Color;
use xui::theme::Theme;
use xui_code_editor::{Highlighter, LineState, Token, TokenClass};

use super::script_lexer::ScriptColours;
use crate::pages::{Block, Page};

/// What a link in the page starts with, when it asks for an example.
const INSERT: &str = "insert:";

/// What a page's examples are: script to offer for the script pane, or text
/// shown as it is, with no link.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Examples {
    Script,
    Plain,
}

/// A page as HTML, and the examples its links stand for.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct HtmlPage {
    pub html: String,
    /// The code of each example, in the order `insert:N` counts them, ending in
    /// the newline that makes it whole statements.
    pub examples: Vec<String>,
}

/// The example a link asks for, if it is one of ours.
pub(super) fn example_of(href: &str) -> Option<usize> {
    href.strip_prefix(INSERT)?.parse().ok()
}

fn hex(c: Color) -> String {
    format!("#{:02x}{:02x}{:02x}", c.r, c.g, c.b)
}

/// The colour the page is drawn on.
pub(super) fn background(theme: &Theme) -> Color {
    theme.input_background
}

/// The stylesheet: the theme's colours, and the editor's colours for code.
fn style(theme: &Theme) -> String {
    let (keyword, string, comment, number) = if theme.is_dark {
        ("#d291e4", "#98c379", "#8b949e", "#e5a35c")
    } else {
        ("#a626a4", "#2e7d32", "#6a737d", "#b35900")
    };
    format!(
        "body {{ margin: 10px 14px; font-family: 'Segoe UI', sans-serif; font-size: 14px; \
         line-height: 1.45; color: {text}; background-color: {bg}; }}\n\
         h1 {{ font-size: 22px; margin: 4px 0 10px 0; }}\n\
         h2 {{ font-size: 17px; margin: 16px 0 6px 0; }}\n\
         p {{ margin: 6px 0; }}\n\
         ul, ol {{ margin: 6px 0; padding-left: 24px; }}\n\
         li {{ margin: 3px 0; }}\n\
         code {{ font-family: Consolas, monospace; font-size: 13px; background-color: {surface}; }}\n\
         pre {{ font-family: Consolas, monospace; font-size: 13px; margin: 8px 0 4px 0; \
         padding: 8px 10px; background-color: {surface}; }}\n\
         a {{ color: {accent}; }}\n\
         p.act {{ margin: 2px 0 10px 0; font-size: 12px; }}\n\
         table {{ border-collapse: collapse; width: 100%; margin: 4px 0 10px 0; }}\n\
         td {{ padding: 3px 8px; vertical-align: middle; line-height: 20px; \
         border: 1px solid {border}; }}\n\
         th {{ padding: 3px 8px; text-align: left; border: 1px solid {border}; \
         background-color: {surface}; }}\n\
         td.sel {{ width: 280px; font-family: Consolas, monospace; font-size: 13px; \
         color: {accent}; }}\n\
         .k {{ color: {keyword}; }} .f {{ color: {accent}; }} .s {{ color: {string}; }}\n\
         .c {{ color: {comment}; }} .n {{ color: {number}; }}\n",
        text = hex(theme.text),
        bg = hex(background(theme)),
        surface = hex(theme.surface),
        border = hex(theme.border),
        accent = hex(theme.accent),
    )
}

fn escape_into(out: &mut String, text: &str) {
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            c => out.push(c),
        }
    }
}

/// Where the closing `marker` of a span opened just before `rest` is.
fn close(rest: &str, marker: &str) -> Option<usize> {
    rest.find(marker).filter(|at| *at > 0)
}

/// Text with `code` and **bold**; a marker nothing closes is only text.
fn inline(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(at) = rest.find(['`', '*']) {
        let (before, from) = rest.split_at(at);
        escape_into(&mut out, before);
        let (marker, tag) = if from.starts_with("**") {
            ("**", "b")
        } else if from.starts_with('`') {
            ("`", "code")
        } else {
            escape_into(&mut out, "*");
            rest = &from[1..];
            continue;
        };
        let inner = &from[marker.len()..];
        match close(inner, marker) {
            Some(end) => {
                out.push_str(&format!("<{tag}>"));
                escape_into(&mut out, &inner[..end]);
                out.push_str(&format!("</{tag}>"));
                rest = &inner[end + marker.len()..];
            }
            None => {
                escape_into(&mut out, marker);
                rest = inner;
            }
        }
    }
    escape_into(&mut out, rest);
    out
}

/// The CSS class a kind of token is drawn in; plain words have none.
fn class_of(class: TokenClass) -> Option<char> {
    match class {
        TokenClass::Keyword => Some('k'),
        TokenClass::Function => Some('f'),
        TokenClass::String => Some('s'),
        TokenClass::Comment => Some('c'),
        TokenClass::Number => Some('n'),
        _ => None,
    }
}

fn paint_line(out: &mut String, line: &str, tokens: &[Token]) {
    let chars: Vec<char> = line.chars().collect();
    let mut at = 0;
    for token in tokens {
        let (start, end) = (token.start.min(chars.len()), token.end.min(chars.len()));
        if start < at || start >= end {
            continue;
        }
        let gap: String = chars[at..start].iter().collect();
        escape_into(out, &gap);
        let word: String = chars[start..end].iter().collect();
        match class_of(token.class) {
            Some(class) => {
                out.push_str(&format!("<span class=\"{class}\">"));
                escape_into(out, &word);
                out.push_str("</span>");
            }
            None => escape_into(out, &word),
        }
        at = end;
    }
    let tail: String = chars[at..].iter().collect();
    escape_into(out, &tail);
}

/// An example coloured as the code editor colours it.
fn highlight(code: &str) -> String {
    let mut out = String::new();
    let mut state = LineState::from_raw(0);
    for (i, line) in code.lines().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        let (tokens, next) = ScriptColours.lex_line(line, &state);
        paint_line(&mut out, line, &tokens);
        state = next;
    }
    out
}

fn list(out: &mut String, ordered: bool, items: &[String]) {
    let tag = if ordered { "ol" } else { "ul" };
    out.push_str(&format!("<{tag}>"));
    for item in items {
        out.push_str(&format!("<li>{}</li>", inline(item)));
    }
    out.push_str(&format!("</{tag}>\n"));
}

/// The page as HTML.
pub(super) fn render(page: &Page, theme: &Theme, kind: Examples) -> HtmlPage {
    let mut html = format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><style>{}</style></head><body>\n<h1>",
        style(theme)
    );
    escape_into(&mut html, &page.title);
    html.push_str("</h1>\n");
    let mut examples = Vec::new();
    let mut in_table = false;
    for block in &page.blocks {
        // Rows in a run are one table, which anything else ends.
        let is_row = matches!(block, Block::Row(..) | Block::Head(..));
        if is_row != in_table {
            html.push_str(if is_row { "<table>\n" } else { "</table>\n" });
            in_table = is_row;
        }
        match block {
            Block::Heading(text) => html.push_str(&format!("<h2>{}</h2>\n", inline(text))),
            Block::Text(text) => html.push_str(&format!("<p>{}</p>\n", inline(text))),
            Block::List { ordered, items } => list(&mut html, *ordered, items),
            Block::Code(code) if kind == Examples::Plain => {
                html.push_str("<pre>");
                escape_into(&mut html, code);
                html.push_str("</pre>\n");
            }
            Block::Code(code) => {
                html.push_str(&format!("<pre>{}</pre>\n", highlight(code)));
                html.push_str(&format!(
                    "<p class=\"act\"><a href=\"{INSERT}{}\">Insert in script</a></p>\n",
                    examples.len()
                ));
                examples.push(format!("{code}\n"));
            }
            Block::Head(a, b) => html.push_str(&format!(
                "<tr><th>{}</th><th>{}</th></tr>\n",
                inline(a),
                inline(b)
            )),
            Block::Row(selector, help) => {
                html.push_str("<tr><td class=\"sel\">");
                escape_into(&mut html, selector);
                html.push_str("</td><td>");
                escape_into(&mut html, help);
                html.push_str("</td></tr>\n");
            }
        }
    }
    if in_table {
        html.push_str("</table>\n");
    }
    html.push_str("</body></html>");
    HtmlPage { html, examples }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::script::guide;
    use Examples::{Plain, Script};

    fn page(blocks: Vec<Block>) -> Page {
        Page {
            title: "T & <U>".into(),
            blocks,
        }
    }

    #[test]
    fn text_is_escaped_and_only_matched_markers_make_code_and_bold() {
        assert_eq!(inline("a < b & c"), "a &lt; b &amp; c");
        assert_eq!(
            inline("use `x < 1` and **now**"),
            "use <code>x &lt; 1</code> and <b>now</b>"
        );
        assert_eq!(inline("2 * 3 `open"), "2 * 3 `open");
        assert_eq!(inline("**"), "**");
        assert_eq!(
            inline("<script>alert(1)</script>"),
            "&lt;script&gt;alert(1)&lt;/script&gt;"
        );
    }

    #[test]
    fn an_example_is_coloured_escaped_and_offered_for_insertion() {
        let html = render(
            &page(vec![
                Block::Code("x := 1 < 2.".into()),
                Block::Code("say: 'hi'.".into()),
            ]),
            &Theme::light(),
            Script,
        );
        assert_eq!(html.examples, ["x := 1 < 2.\n", "say: 'hi'.\n"]);
        assert!(html.html.contains(" &lt; "), "{}", html.html);
        assert!(!html.html.contains(" < "), "{}", html.html);
        assert!(html.html.contains("<span class=\"f\">say:</span>"));
        assert!(html.html.contains("<span class=\"s\">'hi'</span>"));
        assert!(html.html.contains("<h1>T &amp; &lt;U&gt;</h1>"));
        assert!(html.html.contains("href=\"insert:1\""));
    }

    #[test]
    fn a_plain_example_is_escaped_and_offers_nothing() {
        let html = render(
            &page(vec![Block::Code("Ctrl+<wheel> & drag".into())]),
            &Theme::light(),
            Plain,
        );
        assert!(html.examples.is_empty());
        assert!(!html.html.contains("href="), "{}", html.html);
        assert!(
            html.html
                .contains("<pre>Ctrl+&lt;wheel&gt; &amp; drag</pre>")
        );
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
    fn reference_rows_are_a_table_and_the_page_follows_the_theme() {
        let rows = vec![
            Block::Heading("Zone".into()),
            Block::Row("a:".into(), "one".into()),
            Block::Row("b:".into(), "two".into()),
            Block::Text("After.".into()),
        ];
        let light = render(&page(rows.clone()), &Theme::light(), Script);
        let dark = render(&page(rows), &Theme::dark(), Script);
        assert_eq!(light.html.matches("<tr>").count(), 2);
        assert_eq!(light.html.matches("<table>").count(), 1);
        assert_eq!(light.html.matches("</table>").count(), 1);
        assert!(light.html.contains("<td class=\"sel\">b:</td><td>two</td>"));
        assert!(light.html.contains(&hex(Theme::light().input_background)));
        assert!(dark.html.contains(&hex(Theme::dark().input_background)));
        assert_ne!(light.html, dark.html);
    }

    #[test]
    fn every_page_makes_html_with_a_link_for_each_example() {
        for page in guide::pages() {
            let made = render(&page, &Theme::dark(), Script);
            let codes = page
                .blocks
                .iter()
                .filter(|b| matches!(b, Block::Code(_)))
                .count();
            assert_eq!(made.examples.len(), codes, "{}", page.title);
            assert_eq!(made.html.matches("href=").count(), codes, "{}", page.title);
        }
    }

    #[test]
    fn the_reference_lists_every_message() {
        let pages = guide::pages();
        let made = render(
            pages.last().expect("a reference page"),
            &Theme::light(),
            Script,
        );
        for entry in crate::script::api::API {
            let mut escaped = String::new();
            escape_into(&mut escaped, entry.selector);
            assert!(made.html.contains(&escaped), "{}", entry.selector);
        }
    }
}
