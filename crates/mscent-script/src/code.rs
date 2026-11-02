//! Bytecode and the immutable program a loaded script becomes.

use crate::builtin::Builtin;
use crate::heap::Value;
use std::sync::Arc;

/// Limits the compiler enforces. Each is a load error, never a wrapped index.
pub const MAX_CONSTS: usize = 65_535;
pub const MAX_SELECTORS: usize = 65_535;
pub const MAX_CODES: usize = 65_535;
/// Variables in one scope. Every call allocates an environment this big, so
/// the cap also bounds what a call can allocate.
pub const MAX_SLOTS: usize = 1024;
pub const MAX_ARGS: usize = 255;
/// Items in an array literal.
pub const MAX_LITERAL_ITEMS: usize = 1024;

#[derive(Clone, Copy, Debug)]
pub(crate) enum Op {
    Const(u16),
    Nil,
    True,
    False,
    /// Scope depth (0 is the innermost environment) and slot.
    Load(u8, u16),
    Store(u8, u16),
    Pop,
    Send(u16, u8),
    Block(u16),
    Array(u16),
    Jump(u32),
    JumpIfFalse(u32),
    JumpIfTrue(u32),
    Ret,
    /// `^`: return from the method this block was written in.
    RetHome,
    /// `^` outside any method: this handler or top-level body is finished.
    EndTask,
}

#[derive(Debug)]
pub(crate) struct Code {
    pub ops: Vec<Op>,
    /// Source line of each op.
    pub lines: Vec<u32>,
    pub file: u16,
    /// Line where the function starts, for errors with no running op.
    pub line: u32,
    pub nslots: u16,
    pub nparams: u8,
    pub has_recv: bool,
}

/// What a selector does, resolved once at load.
pub(crate) enum Impl<H> {
    Method(u16),
    Native(crate::vm::Native<H>),
    Builtin(Builtin),
    BlockValue,
    Fork,
    Wait,
}

/// Everything a loaded script needs that never changes while it runs, so
/// cloning a `Vm` shares it.
pub(crate) struct Program<H> {
    pub codes: Vec<Code>,
    pub consts: Vec<Value>,
    pub impls: Vec<Impl<H>>,
    pub files: Vec<Arc<str>>,
    /// One top-level body per source that has statements.
    pub entries: Vec<u16>,
    pub native_names: Vec<String>,
}
