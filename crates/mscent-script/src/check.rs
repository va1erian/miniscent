//! Load-time checks: every selector must exist, level names must exist, and
//! the error says what the likely mistake was.

use crate::ast::{Expr, Msg};
use crate::builtin::{BUILTINS, INLINE_SELECTORS, VM_SELECTORS};
use crate::compile::Compiler;
use crate::error::{MAX_LOAD_ERRORS, shorten};
use std::collections::{HashMap, HashSet};

pub(crate) struct MethodMeta {
    pub recv: bool,
    pub file: u16,
    pub line: u32,
}

/// Most keyword parts a selector is split at for the missing-period hint. Far
/// more than any real pair of messages, and each split copies the selector, so
/// a hostile one of a million bytes must not get here.
const MAX_SPLIT_PARTS: usize = 16;

/// Every selector a script may send: natives, the language's own and the
/// script's `define`s.
pub(crate) struct Known<'a> {
    pub natives: HashSet<&'a str>,
    pub methods: HashMap<&'a str, MethodMeta>,
}

fn is_language(sel: &str) -> bool {
    BUILTINS.iter().any(|b| b.0 == sel)
        || VM_SELECTORS.contains(&sel)
        || INLINE_SELECTORS.contains(&sel)
}

impl Known<'_> {
    /// True when `sel` collides with the language, so nothing may redefine it.
    pub(crate) fn reserved(sel: &str) -> bool {
        is_language(sel)
    }

    pub(crate) fn knows(&self, sel: &str) -> bool {
        self.natives.contains(sel) || self.methods.contains_key(sel) || is_language(sel)
    }

    fn names(&self) -> impl Iterator<Item = &str> {
        let lang = BUILTINS
            .iter()
            .map(|b| b.0)
            .chain(VM_SELECTORS.iter().copied())
            .chain(INLINE_SELECTORS.iter().copied());
        self.natives
            .iter()
            .copied()
            .chain(self.methods.keys().copied())
            .chain(lang)
    }
}

fn edit_distance(a: &str, b: &str) -> usize {
    let b: Vec<u8> = b.bytes().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.bytes().enumerate() {
        let mut cur = Vec::with_capacity(prev.len());
        cur.push(i + 1);
        for (j, &cb) in b.iter().enumerate() {
            let diag = prev.get(j).copied().unwrap_or(0) + usize::from(ca != cb);
            let up = prev.get(j + 1).copied().unwrap_or(0) + 1;
            let left = cur.get(j).copied().unwrap_or(0) + 1;
            cur.push(diag.min(up).min(left));
        }
        prev = cur;
    }
    prev.last().copied().unwrap_or(0)
}

/// The known name nearest to `s`, when it is near enough to be a typo.
fn closest<'a>(s: &str, known: impl Iterator<Item = &'a str>) -> Option<&'a str> {
    if s.len() > 64 {
        return None;
    }
    let limit = (s.len() / 3).max(2);
    known
        .map(|k| (edit_distance(s, k), k))
        .filter(|&(d, _)| d <= limit)
        .min_by_key(|&(d, k)| (d, k))
        .map(|(_, k)| k)
}

fn hint(h: Option<&str>) -> String {
    h.map_or(String::new(), |c| format!(" (did you mean '{c}'?)"))
}

impl Compiler<'_> {
    /// Reports what is wrong with a send, if anything. False means the
    /// selector does not exist.
    pub(crate) fn check_send(&mut self, m: &Msg, has_recv: bool, bare: bool) -> bool {
        let sel = m.sel.as_str();
        if !self.known.knows(sel) {
            // Past the cap the message would be dropped, and working out its
            // hint is the expensive part of a load full of errors.
            if self.soft.len() < MAX_LOAD_ERRORS {
                let msg = self.unknown_message(m, bare);
                self.soft_error(msg);
            }
            return false;
        }
        if let Some(meta) = self.known.methods.get(sel) {
            if meta.recv != has_recv {
                let msg = receiver_message(sel, meta.recv, meta.line);
                self.soft_error(msg);
            }
        } else if INLINE_SELECTORS.contains(&sel) {
            let msg = format!("'{sel}' needs literal blocks written in place, like {sel} [ ... ]");
            self.soft_error(msg);
        }
        self.check_level_name(m);
        true
    }

    fn check_level_name(&mut self, m: &Msg) {
        let Some(allowed) = self.names.get(m.sel.as_str()).copied() else {
            return;
        };
        let Some(Expr::Str(name, _)) = m.args.first() else {
            return;
        };
        if allowed.contains(&name.as_str()) || self.soft.len() >= MAX_LOAD_ERRORS {
            return;
        }
        let h = hint(closest(name, allowed.iter().copied()));
        let msg = format!(
            "{} '{}': the level has no such name{h}",
            m.sel,
            shorten(name)
        );
        self.soft_error(msg);
    }

    fn unknown_message(&self, m: &Msg, bare: bool) -> String {
        let sel = shorten(&m.sel);
        if bare {
            let h = hint(closest(&m.sel, self.known.names()));
            return format!(
                "'{sel}' is not a variable or a function; a variable must be assigned before it is read{h}"
            );
        }
        if let Some(msg) = self.missing_period(m) {
            return msg;
        }
        format!(
            "nothing understands '{sel}'{}",
            hint(closest(&m.sel, self.known.names()))
        )
    }

    /// `say: 'a'` newline `say: 'd'.` parses as one message `say:say:`. When
    /// an unknown keyword selector splits into known messages (or every part
    /// is known on its own), a period is probably missing, so say where.
    fn missing_period(&self, m: &Msg) -> Option<String> {
        let parts: Vec<&str> = m.sel.split_inclusive(':').collect();
        if !(2..=MAX_SPLIT_PARTS).contains(&parts.len()) {
            return None;
        }
        let split = (1..parts.len()).find(|&k| {
            let (left, right) = (
                parts.get(..k).unwrap_or(&[]).concat(),
                parts.get(k..).unwrap_or(&[]).concat(),
            );
            self.known.knows(&left) && self.known.knows(&right)
        });
        let k = match split {
            Some(k) => k,
            None if parts.iter().all(|p| self.known.knows(p)) => 1,
            None => return None,
        };
        let part = shorten(parts.get(k).copied().unwrap_or(""));
        let line = m.part_lines.get(k).copied().unwrap_or(m.line);
        Some(format!(
            "nothing understands '{}', but each of its parts is a known message, so a '.' is probably missing before '{part}' on line {line}",
            shorten(&m.sel)
        ))
    }
}

fn receiver_message(sel: &str, wants_recv: bool, line: u32) -> String {
    if wants_recv {
        format!(
            "'{sel}' is defined (line {line} of its file) to be sent to something, but it was written with no receiver"
        )
    } else {
        format!(
            "'{sel}' is a plain function (defined on line {line} of its file), so it takes no receiver"
        )
    }
}
