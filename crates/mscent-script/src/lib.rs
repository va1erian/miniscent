//! A small Smalltalk-style scripting language and its VM, for level logic.
//!
//! The crate depends on nothing and knows nothing about the game. A host type
//! `H` supplies natives (`Native<H>`) and shows the collector what it holds
//! (`Host::roots`). Scripts are untrusted: every limit is checked, nothing here
//! panics on any input, and the op budget counts operations, so replays stop
//! in the same place. See `docs/scripting.md`.
//!
//! The lints below make "cannot panic" checkable: no unwrap, expect or
//! slice indexing outside tests.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

mod ast;
mod builtin;
mod check;
mod code;
mod compile;
mod compile_expr;
mod error;
mod exec;
mod heap;
mod lex;
mod load;
mod parse;
mod prelude;
mod sched;
mod vm;

pub use builtin::language_selectors;
pub use code::{MAX_ARGS, MAX_CODES, MAX_CONSTS, MAX_LITERAL_ITEMS, MAX_SELECTORS, MAX_SLOTS};
pub use error::{MAX_LOAD_ERRORS, ScriptError};
pub use heap::{Heap, MAX_ARRAY_LEN, MAX_HEAP_BYTES, MAX_STR_LEN, Ref, Value};
pub use lex::MAX_SOURCE;
pub use load::{Loaded, NameCheck, Source};
pub use vm::{Flow, Host, MAX_TASKS, Native, OPS_PER_RUN, RUNS_PER_STEP, STEP_HZ, Vm};
