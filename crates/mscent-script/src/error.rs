//! Errors and warnings, always with the source file and line they belong to.

use std::fmt;
use std::sync::Arc;

/// Load errors stop at this many, so a hostile 1 MiB script cannot make a
/// report of a hundred thousand lines.
pub const MAX_LOAD_ERRORS: usize = 50;

/// A load error, a warning or a runtime error. `Display` is `file:line: message`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScriptError {
    /// The source's name as the host gave it to `Vm::load`.
    pub file: Arc<str>,
    /// 1-based line in that file, or 0 when no script line is involved.
    pub line: u32,
    pub msg: String,
    /// The task that failed, for runtime errors, so a host can disable its handler.
    pub task: Option<u64>,
}

impl fmt::Display for ScriptError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}: {}", self.file, self.line, self.msg)
    }
}

/// A problem found while loading, before the file names are attached.
#[derive(Clone, Debug)]
pub(crate) struct Diag {
    pub file: u16,
    pub line: u32,
    pub msg: String,
}

impl Diag {
    pub(crate) fn new(file: u16, line: u32, msg: impl Into<String>) -> Self {
        Self {
            file,
            line,
            msg: msg.into(),
        }
    }

    pub(crate) fn resolve(self, files: &[Arc<str>]) -> ScriptError {
        let file = files
            .get(usize::from(self.file))
            .cloned()
            .unwrap_or_else(|| Arc::from("<unknown>"));
        ScriptError {
            file,
            line: self.line,
            msg: self.msg,
            task: None,
        }
    }
}

/// Cuts a name taken from a script down for an error message, so one hostile
/// identifier cannot make a megabyte of error text.
pub(crate) fn shorten(s: &str) -> String {
    const LIMIT: usize = 40;
    let mut out: String = s.chars().take(LIMIT).collect();
    if out.len() < s.len() {
        out.push_str("...");
    }
    out
}
