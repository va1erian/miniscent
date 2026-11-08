//! A page of the scripting guide or of the editor manual as Markdown, for
//! `xui-markdown` to show, and the script lexer as its code highlighter.
//!
//! The page is one view, so it costs the window's re-flow nothing, where the
//! same words as separate widgets cost 100 ms. The Markdown is made from the
//! pages' own small subset ([`crate::pages`]): inline `code` and **bold** keep
//! their meaning, and every other character that Markdown would read as markup
//! is escaped, so a page shows what its text says. The only links are the ones
//! made here, `insert:N` for the page's Nth example, and `example_of` is the
//! only thing that reads one back: anything else a click reports is ignored, so
//! the page can reach nothing.

use xui::Color;
use xui::Dip;
use xui::theme::Theme;
use xui_code_editor::{Highlighter as _, LineState, TokenClass};
use xui_markdown::{CodeClass, CodeColors, CodePalette, CodeSpan, Highlighter, MarkdownStyle};

use super::script_lexer::ScriptColours;
use crate::pages::{Block, Page};

/// What a link in the page starts with, when it asks for an example.
const INSERT: &str = "insert:";
/// The info string of a script example, which [`ScriptHighlight`] colours.
const SCRIPT: &str = "script";

/// What a page's examples are: script to offer for the script pane, or text
/// shown as it is, with no link.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Examples {
    Script,
    Plain,
}

/// A page as Markdown, and the examples its links stand for.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct MdPage {
    pub markdown: String,
    /// The code of each example, in the order `insert:N` counts them, ending in
    /// the newline that makes it whole statements.
    pub examples: Vec<String>,
}

/// The example a link asks for, if it is one of ours.
pub(super) fn example_of(href: &str) -> Option<usize> {
    href.strip_prefix(INSERT)?.parse().ok()
}

/// The page's fonts and spacing: the code editor's monospace, and headings a
/// step above the text as the help has always had them.
pub(super) fn style(mono: &str) -> MarkdownStyle {
    let body = 14.0;
    MarkdownStyle {
        code_family: Some(mono.to_owned()),
        size: Dip(body),
        code_size: Dip(13.0),
        heading_scale: [22.0 / body, 17.0 / body, 15.0 / body, 1.0, 1.0, 1.0],
        padding: Dip(12.0),
        ..MarkdownStyle::default()
    }
}

/// The script pane's colours for code, with names in the theme's accent.
pub(super) fn palette(theme: &Theme) -> CodePalette {
    let colors = |keyword, string, comment, number| CodeColors {
        keyword: Color::hex(keyword),
        function: theme.accent,
        string: Color::hex(string),
        comment: Color::hex(comment),
        number: Color::hex(number),
        type_name: theme.accent,
    };
    CodePalette {
        light: colors(0xa626a4, 0x2e7d32, 0x6a737d, 0xb35900),
        dark: colors(0xd291e4, 0x98c379, 0x8b949e, 0xe5a35c),
    }
}

/// The script lexer, as the page view's highlighter for `script` examples.
pub(super) struct ScriptHighlight;

impl Highlighter for ScriptHighlight {
    fn line(&self, lang: &str, line: &str, state: u32) -> (Vec<CodeSpan>, u32) {
        if lang != SCRIPT {
            return (Vec::new(), 0);
        }
        let (tokens, next) = ScriptColours.lex_line(line, &LineState::from_raw(u64::from(state)));
        // The lexer counts chars; the view wants bytes.
        let bytes: Vec<usize> = line
            .char_indices()
            .map(|(at, _)| at)
            .chain([line.len()])
            .collect();
        let at = |char: usize| bytes.get(char).copied().unwrap_or(line.len());
        let spans = tokens
            .iter()
            .filter_map(|t| {
                Some(CodeSpan {
                    range: at(t.start)..at(t.end),
                    class: class_of(t.class)?,
                })
            })
            .collect();
        // A state too big for the view starts the next line afresh.
        (spans, u32::try_from(next.as_raw()).unwrap_or(0))
    }
}

/// The view's class for a kind of token; plain words have none.
fn class_of(class: TokenClass) -> Option<CodeClass> {
    match class {
        TokenClass::Keyword => Some(CodeClass::Keyword),
        TokenClass::Function => Some(CodeClass::Function),
        TokenClass::String => Some(CodeClass::String),
        TokenClass::Comment => Some(CodeClass::Comment),
        TokenClass::Number => Some(CodeClass::Number),
        _ => None,
    }
}

/// Escapes what Markdown would read as markup, so `text` shows as written.
/// Any ASCII punctuation may be backslash-escaped; these are the ones that
/// start something anywhere in a line.
fn escape_into(out: &mut String, text: &str) {
    for c in text.chars() {
        if matches!(
            c,
            '\\' | '`' | '*' | '_' | '[' | ']' | '<' | '>' | '|' | '#' | '~' | '!' | '&'
        ) {
            out.push('\\');
        }
        out.push(c);
    }
}

/// Where the closing `marker` of a span opened just before `rest` is.
fn close(rest: &str, marker: &str) -> Option<usize> {
    rest.find(marker).filter(|at| *at > 0)
}

/// The pages' inline subset as Markdown: matched `code` and **bold** keep
/// their meaning, and a marker nothing closes is only text.
fn inline(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    // A line that starts like a list item or a setext underline is text. A
    // backslash escapes only punctuation, so a number keeps its digits and the
    // `.` or `)` after them is escaped.
    if text.starts_with(['-', '+', '=']) {
        out.push('\\');
    } else if let Some(digits) = numbered(text) {
        out.push_str(&text[..digits]);
        out.push('\\');
        rest = &text[digits..];
    }
    while let Some(at) = rest.find(['`', '*']) {
        let (before, from) = rest.split_at(at);
        escape_into(&mut out, before);
        let marker = if from.starts_with("**") {
            "**"
        } else if from.starts_with('`') {
            "`"
        } else {
            escape_into(&mut out, "*");
            rest = &from[1..];
            continue;
        };
        let inner = &from[marker.len()..];
        match close(inner, marker) {
            Some(end) if marker == "`" => {
                // A code span is verbatim; the subset's spans hold no backtick.
                out.push('`');
                out.push_str(&inner[..end]);
                out.push('`');
                rest = &inner[end + 1..];
            }
            Some(end) => {
                out.push_str("**");
                escape_into(&mut out, &inner[..end]);
                out.push_str("**");
                rest = &inner[end + 2..];
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

/// How many digits start `text` when it starts like an ordered list item
/// (`12.` or `12)`).
fn numbered(text: &str) -> Option<usize> {
    let digits = text.chars().take_while(char::is_ascii_digit).count();
    (digits > 0 && text[digits..].starts_with(['.', ')'])).then_some(digits)
}

/// A fence of backticks longer than any run inside `code`.
fn fence(code: &str) -> String {
    let longest = code.split(|c| c != '`').map(str::len).max().unwrap_or(0);
    "`".repeat(longest.max(2) + 1)
}

fn code_block(out: &mut String, code: &str, lang: &str) {
    let fence = fence(code);
    out.push_str(&format!("{fence}{lang}\n{code}\n{fence}\n\n"));
}

/// A table cell holding a name, set as code when it can be.
fn name_cell(name: &str) -> String {
    if name.is_empty() || name.contains('`') {
        let mut out = String::new();
        escape_into(&mut out, name);
        return out;
    }
    // A pipe ends a cell even inside code, unless escaped.
    format!("`{}`", name.replace('|', "\\|"))
}

/// The page as Markdown.
pub(super) fn render(page: &Page, kind: Examples) -> MdPage {
    let mut out = String::from("# ");
    escape_into(&mut out, &page.title);
    out.push_str("\n\n");
    let mut examples = Vec::new();
    let mut in_table = false;
    for block in &page.blocks {
        // Rows in a run are one table, which anything else ends.
        let is_row = matches!(block, Block::Row(..) | Block::Head(..));
        if in_table && !is_row {
            out.push('\n');
        }
        if is_row && !in_table && matches!(block, Block::Row(..)) {
            // Markdown tables need a head; a run with none gets an empty one.
            out.push_str("| | |\n|---|---|\n");
        }
        in_table = is_row;
        match block {
            Block::Heading(text) => out.push_str(&format!("## {}\n\n", inline(text))),
            Block::Text(text) => out.push_str(&format!("{}\n\n", inline(text))),
            Block::List { ordered, items } => {
                for (i, item) in items.iter().enumerate() {
                    let marker = if *ordered {
                        format!("{}.", i + 1)
                    } else {
                        "-".into()
                    };
                    out.push_str(&format!("{marker} {}\n", inline(item)));
                }
                out.push('\n');
            }
            Block::Code(code) if kind == Examples::Plain => code_block(&mut out, code, ""),
            Block::Code(code) => {
                code_block(&mut out, code, SCRIPT);
                out.push_str(&format!(
                    "[Insert in script]({INSERT}{})\n\n",
                    examples.len()
                ));
                examples.push(format!("{code}\n"));
            }
            Block::Head(a, b) => {
                out.push_str(&format!("| {} | {} |\n|---|---|\n", inline(a), inline(b)));
            }
            Block::Row(name, help) => {
                let mut cell = String::new();
                escape_into(&mut cell, help);
                out.push_str(&format!("| {} | {cell} |\n", name_cell(name)));
            }
        }
    }
    MdPage {
        markdown: out,
        examples,
    }
}

#[cfg(test)]
#[path = "guide_md_tests.rs"]
mod tests;
