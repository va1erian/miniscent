//! Pages of help text from Markdown, a small subset of it: `# ` starts a page,
//! `## ` a heading, `- ` and `1. ` list items, `| a | b |` a row of a two-column
//! table (a row over `|---|---|` heads its columns), four spaces an example, and anything else is a paragraph on one line.
//! Inline `code` and **bold** are left in the text for `ui/guide_html.rs` to turn
//! into HTML. The scripting guide and the editor manual are both written in it.
//!
//! It is read here and not by a Markdown crate because the help needs nothing
//! more, and every extra feature is one more thing to escape.

/// A piece of a page.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Block {
    Heading(String),
    Text(String),
    /// Items of a list, as Markdown.
    List {
        ordered: bool,
        items: Vec<String>,
    },
    /// Lines of an example: script in the guide, which the page offers to
    /// insert, or plain text in the manual.
    Code(String),
    /// A row of a two-column table: a name and what it does, as Markdown.
    Row(String, String),
    /// The row over a table's separator: the names of its columns.
    Head(String, String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Page {
    pub title: String,
    pub blocks: Vec<Block>,
}

/// The text of a list item, and whether the list is numbered.
fn list_item(line: &str) -> Option<(bool, &str)> {
    if let Some(item) = line.strip_prefix("- ") {
        return Some((false, item));
    }
    let digits = line.chars().take_while(char::is_ascii_digit).count();
    let item = line[digits..].strip_prefix(". ")?;
    (digits > 0).then_some((true, item))
}

/// A line of a two-column table.
enum TableLine<'a> {
    Row(&'a str, &'a str),
    /// `|---|---|`, under the row that heads the columns.
    Separator,
}

/// What `| a | b |` is; a line without exactly two cells is not one.
fn table_line(line: &str) -> Option<TableLine<'_>> {
    let inner = line.strip_prefix('|')?.strip_suffix('|')?;
    let mut cells = inner.split('|').map(str::trim);
    let (a, b) = (cells.next()?, cells.next()?);
    if cells.next().is_some() {
        return None;
    }
    let separator = |c: &str| !c.is_empty() && c.chars().all(|ch| ch == '-' || ch == ':');
    Some(if separator(a) && separator(b) {
        TableLine::Separator
    } else {
        TableLine::Row(a, b)
    })
}

/// Adds a line of a table: a separator makes the row above it the head.
fn push_table_line(page: &mut Page, line: &str) {
    match table_line(line) {
        Some(TableLine::Row(a, b)) => page.blocks.push(Block::Row(a.to_string(), b.to_string())),
        Some(TableLine::Separator) => {
            if let Some(last) = page.blocks.last_mut()
                && let Block::Row(a, b) = last
            {
                *last = Block::Head(std::mem::take(a), std::mem::take(b));
            }
        }
        // A malformed row is dropped rather than shown as a paragraph of pipes.
        None => {}
    }
}

/// Adds an item to the page's last list if that is a list of its kind.
fn push_item(page: &mut Page, ordered: bool, item: &str) {
    if let Some(Block::List { ordered: o, items }) = page.blocks.last_mut()
        && *o == ordered
    {
        items.push(item.to_string());
        return;
    }
    page.blocks.push(Block::List {
        ordered,
        items: vec![item.to_string()],
    });
}

fn new_page(title: &str) -> Page {
    Page {
        title: title.to_string(),
        blocks: Vec::new(),
    }
}

/// One line that is not part of an example, added to `page`.
fn push_line(page: &mut Page, line: &str) {
    if let Some(heading) = line.strip_prefix("## ") {
        page.blocks.push(Block::Heading(heading.to_string()));
    } else if let Some((ordered, item)) = list_item(line) {
        push_item(page, ordered, item);
    } else if line.starts_with('|') {
        push_table_line(page, line);
    } else if !line.trim().is_empty() {
        page.blocks.push(Block::Text(line.to_string()));
    }
}

/// Pages from Markdown. Lines before the first page are ignored.
pub fn parse(text: &str) -> Vec<Page> {
    let mut pages: Vec<Page> = Vec::new();
    let mut code: Vec<&str> = Vec::new();
    for line in text.lines() {
        let Some(page) = pages.last_mut() else {
            if let Some(title) = line.strip_prefix("# ") {
                pages.push(new_page(title));
            }
            continue;
        };
        if let Some(body) = line.strip_prefix("    ") {
            code.push(body);
            continue;
        }
        if !code.is_empty() {
            page.blocks.push(Block::Code(code.join("\n")));
            code.clear();
        }
        match line.strip_prefix("# ") {
            Some(title) => pages.push(new_page(title)),
            None => push_line(page, line),
        }
    }
    if let (Some(page), false) = (pages.last_mut(), code.is_empty()) {
        page.blocks.push(Block::Code(code.join("\n")));
    }
    pages
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parsing_keeps_code_apart_from_text_and_a_trailing_block_is_kept() {
        let pages = parse(
            "before\n# A\nHello.\n\n    x := 1.\n    y := 2.\n\n## B\nMore.\n- one\n- two\n1. first\n# C\n    z := 3.\n",
        );
        assert_eq!(pages.len(), 2);
        assert_eq!(
            pages[0].blocks,
            [
                Block::Text("Hello.".into()),
                Block::Code("x := 1.\ny := 2.".into()),
                Block::Heading("B".into()),
                Block::Text("More.".into()),
                Block::List {
                    ordered: false,
                    items: vec!["one".into(), "two".into()]
                },
                Block::List {
                    ordered: true,
                    items: vec!["first".into()]
                },
            ]
        );
        assert_eq!(pages[1].blocks, [Block::Code("z := 3.".into())]);
    }

    #[test]
    fn a_number_without_its_period_and_space_is_text() {
        let pages = parse("# A\n3 timesRepeat: x.\n2.5 is a number\n");
        assert_eq!(pages[0].blocks.len(), 2);
        assert!(pages[0].blocks.iter().all(|b| matches!(b, Block::Text(_))));
    }

    #[test]
    fn a_two_cell_table_has_a_head_and_rows_and_odd_rows_are_dropped() {
        let pages = parse(
            "# A\n| Key | What |\n|---|:--|\n| **B** | Box `brush` |\n| one | two | three |\n|x|\n",
        );
        assert_eq!(
            pages[0].blocks,
            [
                Block::Head("Key".into(), "What".into()),
                Block::Row("**B**".into(), "Box `brush`".into()),
            ]
        );
    }
}
