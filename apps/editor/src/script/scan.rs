//! A forgiving look at script text being typed. The compiler stops at the first
//! syntax error, and text being typed is usually wrong somewhere, so completion
//! and colouring read it with these instead: they accept anything.

use super::api::Recv;

/// What the caret is in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum At {
    Comment,
    /// In a string starting at char `start`; `keyword` is the message that takes
    /// it, as in `mover:`.
    String {
        start: usize,
        keyword: Option<String>,
    },
    /// In code, in the word starting at char `start`, after a receiver if one
    /// could be told.
    Code {
        start: usize,
        receiver: Option<Recv>,
    },
}

fn is_word(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// Where the region the end of `chars` is in begins: a string or comment still
/// open there, by its opening quote's offset.
fn open_quote(chars: &[char]) -> Option<(char, usize)> {
    let mut open: Option<(char, usize)> = None;
    for (i, &c) in chars.iter().enumerate() {
        match open {
            Some((quote, _)) if c == quote => open = None,
            Some(_) => {}
            None if c == '"' || c == '\'' => open = Some((c, i)),
            None => {}
        }
    }
    open
}

/// The word that ends at `end`, skipping spaces before it, with a `:` if it has
/// one, and where it starts.
fn word_before(chars: &[char], end: usize) -> Option<(String, usize)> {
    let mut end = end.min(chars.len());
    while end > 0 && chars[end - 1].is_whitespace() {
        end -= 1;
    }
    let body_end = if end > 0 && chars[end - 1] == ':' {
        end - 1
    } else {
        end
    };
    let mut start = body_end;
    while start > 0 && is_word(chars[start - 1]) {
        start -= 1;
    }
    (start < body_end).then(|| (chars[start..end].iter().collect(), start))
}

/// What the expression in the brackets ending at `close` makes, from its first
/// word: `(mover: 'door')` makes a mover.
fn paren_makes(chars: &[char], close: usize) -> Option<Recv> {
    let (mut depth, mut i) = (0i32, close);
    loop {
        match chars[i] {
            ')' => depth += 1,
            '(' => {
                depth -= 1;
                if depth == 0 {
                    break;
                }
            }
            _ => {}
        }
        i = i.checked_sub(1)?;
    }
    let inside: String = chars[i + 1..close].iter().collect();
    Recv::made_by(inside.split_whitespace().next()?)
}

/// What the variable `name` was last assigned, if the text can tell.
fn variable_makes(chars: &[char], before: usize, name: &str) -> Option<Recv> {
    let text: String = chars[..before].iter().collect();
    let mut found = None;
    for line in text.split('.') {
        let Some((left, right)) = line.split_once(":=") else {
            continue;
        };
        if left.trim() == name {
            let first = right.trim_start().trim_start_matches('(');
            found = first.split_whitespace().next().and_then(Recv::made_by);
        }
    }
    found
}

/// What the thing before char `end` is, when a message is being sent to it. Only
/// on the same line: a word at the start of a line is usually a new statement.
fn receiver_before(chars: &[char], end: usize) -> Option<Recv> {
    let mut end = end;
    while end > 0 && chars[end - 1].is_whitespace() && chars[end - 1] != '\n' {
        end -= 1;
    }
    let last = *chars.get(end.checked_sub(1)?)?;
    match last {
        ')' => paren_makes(chars, end - 1),
        ']' => Some(Recv::Block),
        '}' => Some(Recv::Array),
        '\'' => Some(Recv::Value),
        _ if last.is_ascii_digit() => Some(Recv::Value),
        _ if is_word(last) => {
            let (word, _) = word_before(chars, end)?;
            if word == "player" {
                return Some(Recv::Player);
            }
            variable_makes(chars, end, &word)
        }
        _ => None,
    }
}

/// Reads what the caret is in. A caret past the end counts as at the end.
pub fn at_caret(text: &str, caret: usize) -> At {
    let chars: Vec<char> = text.chars().take(caret).collect();
    match open_quote(&chars) {
        Some(('"', _)) => At::Comment,
        Some((_, open)) => At::String {
            start: open + 1,
            keyword: word_before(&chars, open)
                .map(|(word, _)| word)
                .filter(|w| w.ends_with(':')),
        },
        None => {
            let mut start = chars.len();
            while start > 0 && is_word(chars[start - 1]) {
                start -= 1;
            }
            At::Code {
                start,
                receiver: receiver_before(&chars, start),
            }
        }
    }
}

/// The text with its comments blanked out, and its strings too unless asked for.
fn code_only(text: &str, keep_strings: bool) -> String {
    let mut open = None;
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match open {
            Some(q) if c == q => {
                open = None;
                out.push(if q == '"' || !keep_strings { ' ' } else { c });
            }
            Some(q) => out.push(if q == '"' || !keep_strings { ' ' } else { c }),
            None if c == '"' || c == '\'' => {
                open = Some(c);
                out.push(if c == '\'' && keep_strings { c } else { ' ' });
            }
            None => out.push(c),
        }
    }
    out
}

/// Variables the text assigns or a block takes, in order and without repeats.
pub fn assigned_names(text: &str) -> Vec<String> {
    let code = code_only(text, false);
    let mut found: Vec<String> = Vec::new();
    let mut add = |word: &str| {
        if !word.is_empty() && !found.iter().any(|f| f == word) {
            found.push(word.to_string());
        }
    };
    for (i, _) in code.match_indices(":=") {
        let left = code[..i].trim_end();
        let at = left.rfind(|c: char| !is_word(c)).map_or(0, |p| p + 1);
        add(&left[at..]);
    }
    for part in code.split(":").skip(1) {
        let word: String = part.chars().take_while(|c| is_word(*c)).collect();
        // A block's parameter is `[:x`; what follows a keyword is after a space.
        if part.starts_with(|c: char| is_word(c)) {
            add(&word);
        }
    }
    found
}

/// The strings that follow a keyword, as `pose:` in `pose: 'open' move: v`.
pub fn quoted_after(text: &str, keyword: &str) -> Vec<String> {
    let code = code_only(text, true);
    let mut found = Vec::new();
    for (i, _) in code.match_indices(keyword) {
        let after = code[i + keyword.len()..].trim_start();
        if let Some(rest) = after.strip_prefix('\'')
            && let Some(end) = rest.find('\'')
        {
            found.push(rest[..end].to_string());
        }
    }
    found
}

/// A function a script defines.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Define {
    pub selector: String,
    /// What completion inserts: the call, with the parameters' names.
    pub insert: String,
    /// The first line of the comment above it, to show beside it.
    pub note: String,
    pub receiver: bool,
}

/// The comment that closes just before `at`, as its first line.
fn comment_before(text: &str, at: usize) -> String {
    let before = text[..at].trim_end();
    if !before.ends_with('"') {
        return String::new();
    }
    let inner = &before[..before.len() - 1];
    let open = inner.rfind('"').map_or(0, |o| o + 1);
    let first = inner[open..].lines().next().unwrap_or("");
    first.trim().to_string()
}

/// The function a header defines. `define name: a other: b`, `define arr first`
/// and `define arr at: i` are the forms; anything else is `None`.
fn parse_header(header: &str) -> Option<(String, String, bool)> {
    let words: Vec<&str> = header.split_whitespace().collect();
    let first = *words.first()?;
    let receiver = !first.ends_with(':') && words.len() > 1;
    let call = if receiver { &words[1..] } else { &words[..] };
    if call.len() == 1 && !call[0].ends_with(':') {
        return Some((call[0].to_string(), call[0].to_string(), receiver));
    }
    let (mut selector, mut insert) = (String::new(), Vec::new());
    for pair in call.chunks(2) {
        let keyword = *pair.first()?;
        let argument = *pair.get(1)?;
        if !keyword.ends_with(':') {
            return None;
        }
        selector.push_str(keyword);
        insert.push(format!("{keyword} {argument}"));
    }
    Some((selector, insert.join(" "), receiver))
}

/// Every function the text defines. Only a line starting with `define` counts,
/// so text in a comment or string rarely passes for one, and when it does the
/// result is one suggestion too many.
pub fn defines(text: &str) -> Vec<Define> {
    let mut found = Vec::new();
    let mut offset = 0;
    for line in text.split_inclusive('\n') {
        let start = offset;
        offset += line.len();
        let Some(rest) = line.trim_start().strip_prefix("define ") else {
            continue;
        };
        let Some(header) = rest.split('[').next() else {
            continue;
        };
        if let Some((selector, insert, receiver)) = parse_header(header) {
            found.push(Define {
                selector,
                insert,
                note: comment_before(text, start),
                receiver,
            });
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_three_forms_of_define_are_read() {
        let text = "\"Opens a door.\nMore.\"\ndefine open: a in: b [ ]\ndefine arr first [ ]\ndefine arr at2: i [ ]\ndefine go [ ]\n";
        let got = defines(text);
        let row = |i: usize| {
            (
                got[i].selector.as_str(),
                got[i].insert.as_str(),
                got[i].receiver,
            )
        };
        assert_eq!(row(0), ("open:in:", "open: a in: b", false));
        assert_eq!(got[0].note, "Opens a door.");
        assert_eq!(row(1), ("first", "first", true));
        assert_eq!(row(2), ("at2:", "at2: i", true));
        assert_eq!(row(3), ("go", "go", false));
    }

    #[test]
    fn a_broken_define_is_skipped() {
        assert!(defines("define\ndefine [ ]\ndefine a: [ ]\n").is_empty());
    }

    #[test]
    fn variables_are_read_from_assignments_and_blocks_but_not_from_text() {
        let got = assigned_names("a := 1. b:=2. \"c := 3\" 'd := 4'. x do: [:e | e]. a := 5.");
        assert_eq!(got, ["a", "b", "e"]);
    }

    #[test]
    fn a_keyword_is_not_a_block_parameter() {
        assert!(assigned_names("door moveTo: 'open' in: 1.").is_empty());
    }
}
