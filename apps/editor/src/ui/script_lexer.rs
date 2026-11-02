//! Colours for the script language, and the bridge from what the model offers to
//! the code editor's completion popup.

use std::cell::RefCell;
use std::rc::Rc;

use xui_code_editor::{
    Completer, Completion, CompletionItem, CompletionKind, Highlighter, LineState, Token,
    TokenClass,
};

use crate::model::EditorState;
use crate::script::complete::{Kind, Suggestion, suggest};
use crate::script::level_names;
use crate::script::scan::Define;

/// A comment or string can run over several lines.
const CODE: u64 = 0;
const COMMENT: u64 = 1;
const STRING: u64 = 2;

pub struct ScriptColours;

fn is_word(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// Where the region opened at `from` closes, past its quote; the line's end if
/// it goes on.
fn close_of(chars: &[char], from: usize, quote: char) -> (usize, bool) {
    match chars[from..].iter().position(|c| *c == quote) {
        Some(i) => (from + i + 1, true),
        None => (chars.len(), false),
    }
}

/// The word at `i`, its end and what it is. A word and a colon is a message,
/// except the assignment `:=`, which is not part of the word.
fn word_at(chars: &[char], i: usize) -> (usize, TokenClass) {
    let mut end = i;
    while end < chars.len() && is_word(chars[end]) {
        end += 1;
    }
    let text: String = chars[i..end].iter().collect();
    if matches!(text.as_str(), "define" | "true" | "false" | "nil") {
        return (end, TokenClass::Keyword);
    }
    if chars.get(end) == Some(&':') && chars.get(end + 1) != Some(&'=') {
        return (end + 1, TokenClass::Function);
    }
    (end, TokenClass::Identifier)
}

impl Highlighter for ScriptColours {
    fn lex_line(&self, line: &str, state: &LineState) -> (Vec<Token>, LineState) {
        let chars: Vec<char> = line.chars().collect();
        let mut tokens = Vec::new();
        let mut state = state.as_raw();
        let mut i = 0;
        while i < chars.len() {
            let c = chars[i];
            let (end, class) = if state != CODE {
                let (quote, class) = if state == COMMENT {
                    ('"', TokenClass::Comment)
                } else {
                    ('\'', TokenClass::String)
                };
                let (end, closed) = close_of(&chars, i, quote);
                if closed {
                    state = CODE;
                }
                (end, class)
            } else if c.is_whitespace() {
                i += 1;
                continue;
            } else if c == '"' || c == '\'' {
                let (end, closed) = close_of(&chars, i + 1, c);
                if !closed {
                    state = if c == '"' { COMMENT } else { STRING };
                }
                let class = if c == '"' {
                    TokenClass::Comment
                } else {
                    TokenClass::String
                };
                (end, class)
            } else if c.is_ascii_digit() {
                let digits = chars[i..]
                    .iter()
                    .position(|d| !d.is_ascii_digit() && *d != '.');
                let end = digits.map_or(chars.len(), |n| i + n);
                // The period ending `x := 3.` belongs to the statement.
                let end = if chars[end - 1] == '.' { end - 1 } else { end };
                (end, TokenClass::Number)
            } else if is_word(c) {
                word_at(&chars, i)
            } else if "()[]{}.|".contains(c) {
                (i + 1, TokenClass::Punctuation)
            } else if c == ':' && chars.get(i + 1) == Some(&'=') {
                (i + 2, TokenClass::Operator)
            } else {
                (i + 1, TokenClass::Operator)
            };
            tokens.push(Token {
                class,
                start: i,
                end,
            });
            i = end;
        }
        (tokens, LineState::from_raw(state))
    }
}

fn item(s: Suggestion) -> CompletionItem {
    let kind = match s.kind {
        Kind::Function => CompletionKind::Function,
        Kind::Method => CompletionKind::Method,
        Kind::Variable => CompletionKind::Variable,
        Kind::Name => CompletionKind::Property,
    };
    let out = CompletionItem::new(s.label, kind).with_insert(s.insert);
    if s.detail.is_empty() {
        out
    } else {
        out.with_detail(s.detail)
    }
}

/// Offers what the model suggests. It reads the shared state, which nothing holds
/// while a widget handles its input; if something does, it offers nothing.
pub struct ScriptCompleter {
    pub state: Rc<RefCell<EditorState>>,
    pub library: Rc<RefCell<Vec<Define>>>,
}

impl Completer for ScriptCompleter {
    fn complete(&self, text: &str, caret: usize) -> Option<Completion> {
        let state = self.state.try_borrow().ok()?;
        let names = level_names(&state.doc);
        let library = self.library.try_borrow().ok()?;
        let (start, items) = suggest(&names, &library, text, caret)?;
        Some(Completion {
            start,
            items: items.into_iter().map(item).collect(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lex(line: &str, from: u64) -> (Vec<(String, TokenClass)>, u64) {
        let (tokens, next) = ScriptColours.lex_line(line, &LineState::from_raw(from));
        let chars: Vec<char> = line.chars().collect();
        let text = |t: &Token| chars[t.start..t.end].iter().collect::<String>();
        let out = tokens.iter().map(|t| (text(t), t.class)).collect();
        (out, next.as_raw())
    }

    #[test]
    fn a_line_of_script_gets_its_classes() {
        let (tokens, next) = lex("door := mover: 'gate'. \"x\"", CODE);
        assert_eq!(next, CODE);
        assert_eq!(
            tokens,
            [
                ("door".to_string(), TokenClass::Identifier),
                (":=".to_string(), TokenClass::Operator),
                ("mover:".to_string(), TokenClass::Function),
                ("'gate'".to_string(), TokenClass::String),
                (".".to_string(), TokenClass::Punctuation),
                ("\"x\"".to_string(), TokenClass::Comment),
            ]
        );
    }

    #[test]
    fn a_number_keeps_its_point_but_not_the_period_after_it() {
        let (tokens, _) = lex("x := 1.5.", CODE);
        assert_eq!(tokens[2], ("1.5".to_string(), TokenClass::Number));
        assert_eq!(tokens[3], (".".to_string(), TokenClass::Punctuation));
    }

    #[test]
    fn define_and_nil_are_keywords() {
        let (tokens, _) = lex("define go: nil", CODE);
        assert_eq!(tokens[0].1, TokenClass::Keyword);
        assert_eq!(tokens[2].1, TokenClass::Keyword);
    }

    #[test]
    fn comments_and_strings_carry_over_lines() {
        let (tokens, next) = lex("x \"open", CODE);
        assert_eq!(next, COMMENT);
        assert_eq!(tokens[1].1, TokenClass::Comment);
        let (tokens, next) = lex("still it's", COMMENT);
        assert_eq!(next, COMMENT);
        assert_eq!(tokens.len(), 1);
        let (tokens, next) = lex("end\" y", COMMENT);
        assert_eq!(next, CODE);
        assert_eq!(tokens[1], ("y".to_string(), TokenClass::Identifier));
        assert_eq!(lex("'open", CODE).1, STRING);
        assert_eq!(lex("shut' z", STRING).1, CODE);
    }

    #[test]
    fn any_line_lexes_with_spans_in_order() {
        for line in [
            "", "'", "\"", "é'é", "1.", "...", ":=:=", "a:b:", "\t \t", "x:",
        ] {
            for from in [CODE, COMMENT, STRING] {
                let (tokens, _) = ScriptColours.lex_line(line, &LineState::from_raw(from));
                let mut at = 0;
                for t in tokens {
                    assert!(t.start >= at && t.end > t.start, "{line:?} {t:?}");
                    at = t.end;
                }
                assert!(at <= line.chars().count(), "{line:?}");
            }
        }
    }
}
