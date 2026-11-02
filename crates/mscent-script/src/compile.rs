//! Bytecode compiler: scopes, constants, selectors and function buffers. The
//! expression walk is in `compile_expr.rs` and the load-time checks in
//! `check.rs`. Every index that goes into an op is converted with a checked
//! conversion, so a huge script is a load error, never wrong bytecode.

use crate::ast::{Define, Expr};
use crate::check::Known;
use crate::code::{Code, MAX_CODES, MAX_CONSTS, MAX_SELECTORS, MAX_SLOTS, Op};
use crate::error::{Diag, MAX_LOAD_ERRORS, shorten};
use crate::heap::MAX_STR_LEN;
use std::collections::HashMap;

pub(crate) type R<T = ()> = Result<T, Diag>;

pub(crate) enum ConstVal {
    Num(f32),
    Str(String),
}

struct Var {
    name: String,
    file: u16,
    line: u32,
    /// Declared by an assignment, as opposed to a parameter.
    from_assign: bool,
    read: bool,
}

/// One real environment. Inline blocks (`ifTrue: [ ]` and friends) share
/// their parent's scope, so their variables live there too.
#[derive(Default)]
struct Scope {
    vars: Vec<Var>,
    index: HashMap<String, u16>,
}

#[derive(Default)]
struct FnBuf {
    ops: Vec<Op>,
    lines: Vec<u32>,
}

pub(crate) struct Compiler<'a> {
    pub known: &'a Known<'a>,
    pub names: HashMap<&'a str, &'a [&'a str]>,
    pub file: u16,
    pub line: u32,
    pub codes: Vec<Code>,
    pub consts: Vec<ConstVal>,
    num_ids: HashMap<u32, u16>,
    str_ids: HashMap<String, u16>,
    pub sels: Vec<String>,
    sel_ids: HashMap<String, u16>,
    scope: Scope,
    outer: Vec<Scope>,
    buf: FnBuf,
    saved: Vec<FnBuf>,
    /// Errors that do not stop the compile, so one run reports many.
    pub soft: Vec<Diag>,
    pub warnings: Vec<Diag>,
    pub method_code: HashMap<String, u16>,
    pub entries: Vec<u16>,
    pub top_slots: usize,
}

impl<'a> Compiler<'a> {
    pub(crate) fn new(known: &'a Known<'a>, names: HashMap<&'a str, &'a [&'a str]>) -> Self {
        Self {
            known,
            names,
            file: 0,
            line: 1,
            codes: Vec::new(),
            consts: Vec::new(),
            num_ids: HashMap::new(),
            str_ids: HashMap::new(),
            sels: Vec::new(),
            sel_ids: HashMap::new(),
            scope: Scope::default(),
            outer: Vec::new(),
            buf: FnBuf::default(),
            saved: Vec::new(),
            soft: Vec::new(),
            warnings: Vec::new(),
            method_code: HashMap::new(),
            entries: Vec::new(),
            top_slots: 0,
        }
    }

    pub(crate) fn diag(&self, msg: impl Into<String>) -> Diag {
        Diag::new(self.file, self.line, msg)
    }

    pub(crate) fn soft_error(&mut self, msg: String) {
        if self.soft.len() < MAX_LOAD_ERRORS {
            let d = self.diag(msg);
            self.soft.push(d);
        }
    }

    fn limit(&self, what: &str, max: usize) -> Diag {
        self.diag(format!("too many {what}: the limit is {max}"))
    }

    /// Compiles every source's top level, then every define, so defines see
    /// all the top-level variables. The prelude's defines go first: they use
    /// no top-level variable, and compiling them first means a limit blown by
    /// a script is blamed on the script, not on the prelude's few constants.
    pub(crate) fn program(&mut self, sources: &[(u16, &[Expr], &[Define])]) -> R {
        let (prelude, rest) = sources.split_at(sources.len().min(1));
        self.defines(prelude)?;
        for &(file, top, _) in sources {
            self.file = file;
            self.line = 1;
            self.top_body(top)?;
        }
        self.defines(rest)?;
        self.top_slots = self.scope.vars.len();
        let scope = std::mem::take(&mut self.scope);
        self.warn_unread(scope);
        Ok(())
    }

    fn defines(&mut self, sources: &[(u16, &[Expr], &[Define])]) -> R {
        for &(file, _, defines) in sources {
            self.file = file;
            for d in defines {
                self.method(d)?;
            }
        }
        Ok(())
    }

    fn top_body(&mut self, stmts: &[Expr]) -> R {
        if stmts.is_empty() {
            return Ok(());
        }
        self.begin_fn();
        self.body(stmts)?;
        self.emit(Op::Ret);
        // The top environment is sized for the whole program, not per entry.
        let id = self.end_fn(0, false, 1, 0)?;
        self.entries.push(id);
        Ok(())
    }

    fn method(&mut self, d: &Define) -> R {
        self.line = d.line;
        self.enter_scope();
        for n in d.recv.iter().chain(d.params.iter()) {
            self.declare(n, false)?;
        }
        self.begin_fn();
        self.body(&d.body)?;
        self.emit(Op::Ret);
        let nslots = self.scope.vars.len();
        let id = self.end_fn(d.params.len(), d.recv.is_some(), d.line, nslots)?;
        self.leave_scope();
        self.method_code.insert(d.sel.clone(), id);
        Ok(())
    }

    pub(crate) fn real_block(&mut self, params: &[String], body: &[Expr], line: u32) -> R {
        self.enter_scope();
        for p in params {
            self.declare(p, false)?;
        }
        self.begin_fn();
        self.body(body)?;
        self.emit(Op::Ret);
        let nslots = self.scope.vars.len();
        let id = self.end_fn(params.len(), false, line, nslots)?;
        self.leave_scope();
        self.emit(Op::Block(id));
        Ok(())
    }

    fn enter_scope(&mut self) {
        let parent = std::mem::take(&mut self.scope);
        self.outer.push(parent);
    }

    fn leave_scope(&mut self) {
        let parent = self.outer.pop().unwrap_or_default();
        let done = std::mem::replace(&mut self.scope, parent);
        self.warn_unread(done);
    }

    /// A variable assigned and never read is almost always a typo in a name.
    fn warn_unread(&mut self, scope: Scope) {
        for v in scope.vars.iter().filter(|v| v.from_assign && !v.read) {
            let msg = format!(
                "variable '{}' is assigned but never read (a typo in the name?)",
                shorten(&v.name)
            );
            self.warnings.push(Diag::new(v.file, v.line, msg));
        }
    }

    fn begin_fn(&mut self) {
        let old = std::mem::take(&mut self.buf);
        self.saved.push(old);
    }

    fn end_fn(&mut self, nparams: usize, has_recv: bool, line: u32, nslots: usize) -> R<u16> {
        let done = std::mem::replace(&mut self.buf, self.saved.pop().unwrap_or_default());
        if self.codes.len() >= MAX_CODES {
            return Err(self.limit("functions and blocks", MAX_CODES));
        }
        let id = u16::try_from(self.codes.len())
            .map_err(|_| self.limit("functions and blocks", MAX_CODES))?;
        let nslots =
            u16::try_from(nslots).map_err(|_| self.limit("variables in one scope", MAX_SLOTS))?;
        let nparams =
            u8::try_from(nparams).map_err(|_| self.limit("parameters", usize::from(u8::MAX)))?;
        let code = Code {
            ops: done.ops,
            lines: done.lines,
            file: self.file,
            line,
            nslots,
            nparams,
            has_recv,
        };
        self.codes.push(code);
        Ok(id)
    }

    pub(crate) fn emit(&mut self, op: Op) -> usize {
        self.buf.ops.push(op);
        self.buf.lines.push(self.line);
        self.buf.ops.len() - 1
    }

    pub(crate) fn here(&self) -> R<u32> {
        u32::try_from(self.buf.ops.len()).map_err(|_| self.diag("function is too long"))
    }

    pub(crate) fn patch(&mut self, at: usize) -> R {
        let to = self.here()?;
        if let Some(op) = self.buf.ops.get_mut(at) {
            *op = match *op {
                Op::Jump(_) => Op::Jump(to),
                Op::JumpIfFalse(_) => Op::JumpIfFalse(to),
                Op::JumpIfTrue(_) => Op::JumpIfTrue(to),
                other => other,
            };
        }
        Ok(())
    }

    /// Looks a variable up, innermost scope first, as (depth, slot).
    pub(crate) fn resolve(&mut self, name: &str, read: bool) -> R<Option<(u8, u16)>> {
        if let Some(&slot) = self.scope.index.get(name) {
            mark_read(&mut self.scope, slot, read);
            return Ok(Some((0, slot)));
        }
        for (d, scope) in self.outer.iter_mut().rev().enumerate() {
            if let Some(&slot) = scope.index.get(name) {
                mark_read(scope, slot, read);
                let depth = u8::try_from(d + 1)
                    .map_err(|_| Diag::new(self.file, self.line, "scopes nested too deeply"))?;
                return Ok(Some((depth, slot)));
            }
        }
        Ok(None)
    }

    pub(crate) fn declare(&mut self, name: &str, from_assign: bool) -> R<u16> {
        if self.scope.vars.len() >= MAX_SLOTS {
            return Err(self.limit("variables in one scope", MAX_SLOTS));
        }
        let slot = u16::try_from(self.scope.vars.len())
            .map_err(|_| self.limit("variables in one scope", MAX_SLOTS))?;
        let var = Var {
            name: name.to_string(),
            file: self.file,
            line: self.line,
            from_assign,
            read: false,
        };
        self.scope.vars.push(var);
        self.scope.index.insert(name.to_string(), slot);
        Ok(slot)
    }

    pub(crate) fn num_const(&mut self, v: f32) -> R<u16> {
        if let Some(&i) = self.num_ids.get(&v.to_bits()) {
            return Ok(i);
        }
        let i = self.next_const()?;
        self.consts.push(ConstVal::Num(v));
        self.num_ids.insert(v.to_bits(), i);
        Ok(i)
    }

    pub(crate) fn str_const(&mut self, s: &str) -> R<u16> {
        if s.len() > MAX_STR_LEN {
            return Err(self.diag(format!(
                "string literal is {} bytes; the limit is {MAX_STR_LEN}",
                s.len()
            )));
        }
        if let Some(&i) = self.str_ids.get(s) {
            return Ok(i);
        }
        let i = self.next_const()?;
        self.consts.push(ConstVal::Str(s.to_string()));
        self.str_ids.insert(s.to_string(), i);
        Ok(i)
    }

    fn next_const(&self) -> R<u16> {
        if self.consts.len() >= MAX_CONSTS {
            return Err(self.limit("distinct numbers and strings", MAX_CONSTS));
        }
        u16::try_from(self.consts.len())
            .map_err(|_| self.limit("distinct numbers and strings", MAX_CONSTS))
    }

    pub(crate) fn sel(&mut self, s: &str) -> R<u16> {
        if let Some(&i) = self.sel_ids.get(s) {
            return Ok(i);
        }
        if self.sels.len() >= MAX_SELECTORS {
            return Err(self.limit("distinct message names", MAX_SELECTORS));
        }
        let i = u16::try_from(self.sels.len())
            .map_err(|_| self.limit("distinct message names", MAX_SELECTORS))?;
        self.sels.push(s.to_string());
        self.sel_ids.insert(s.to_string(), i);
        Ok(i)
    }
}

fn mark_read(scope: &mut Scope, slot: u16, read: bool) {
    if read && let Some(v) = scope.vars.get_mut(usize::from(slot)) {
        v.read = true;
    }
}
