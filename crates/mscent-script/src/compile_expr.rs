//! Compiles expressions. `ifTrue:`, `whileTrue:` and friends written with
//! literal blocks become jumps, as in Smalltalk; every other message is a send
//! resolved at load.

use crate::ast::{BlockExpr, Expr, Msg, MsgExpr};
use crate::code::{MAX_ARGS, MAX_LITERAL_ITEMS, Op};
use crate::compile::{Compiler, R};

/// A placeholder target, patched once the jump's destination is known.
const TO_PATCH: u32 = 0;

fn literal_block(e: &Expr) -> Option<&BlockExpr> {
    match e {
        Expr::Block(b) if b.params.is_empty() => Some(b),
        _ => None,
    }
}

impl Compiler<'_> {
    /// Statements, leaving the last one's value on the stack.
    pub(crate) fn body(&mut self, stmts: &[Expr]) -> R {
        if stmts.is_empty() {
            self.emit(Op::Nil);
        }
        for (i, s) in stmts.iter().enumerate() {
            self.expr(s)?;
            if i + 1 < stmts.len() {
                self.emit(Op::Pop);
            }
        }
        Ok(())
    }

    pub(crate) fn expr(&mut self, e: &Expr) -> R {
        match e {
            Expr::Num(v, line) => {
                self.line = *line;
                let i = self.num_const(*v)?;
                self.emit(Op::Const(i));
            }
            Expr::Str(s, line) => {
                self.line = *line;
                let i = self.str_const(s)?;
                self.emit(Op::Const(i));
            }
            Expr::Var(name, line) => self.var(name, *line)?,
            Expr::Assign(name, value, line) => {
                self.expr(value)?;
                self.line = *line;
                let (depth, slot) = match self.resolve(name, false)? {
                    Some(found) => found,
                    None => (0, self.declare(name, true)?),
                };
                self.emit(Op::Store(depth, slot));
            }
            Expr::Block(b) => {
                self.line = b.line;
                self.real_block(&b.params, &b.body, b.line)?;
            }
            Expr::Array(items, line) => self.array(items, *line)?,
            Expr::Return(value, line) => {
                self.expr(value)?;
                self.line = *line;
                self.emit(if self.in_method {
                    Op::RetHome
                } else {
                    Op::EndTask
                });
            }
            Expr::Msg(m) => self.msg_expr(m)?,
        }
        Ok(())
    }

    fn var(&mut self, name: &str, line: u32) -> R {
        self.line = line;
        match name {
            "nil" => drop(self.emit(Op::Nil)),
            "true" => drop(self.emit(Op::True)),
            "false" => drop(self.emit(Op::False)),
            _ => match self.resolve(name, true)? {
                Some((d, s)) => drop(self.emit(Op::Load(d, s))),
                // A bare word that is no variable is a global function.
                None => {
                    let m = Msg {
                        sel: name.to_string(),
                        args: Vec::new(),
                        line,
                        part_lines: Vec::new(),
                    };
                    self.send(None, &m, true)?;
                }
            },
        }
        Ok(())
    }

    fn array(&mut self, items: &[Expr], line: u32) -> R {
        self.line = line;
        if items.len() > MAX_LITERAL_ITEMS {
            return Err(self.diag(format!(
                "array literal has {} items; the limit is {MAX_LITERAL_ITEMS}",
                items.len()
            )));
        }
        for it in items {
            self.expr(it)?;
        }
        self.line = line;
        let n = u16::try_from(items.len()).map_err(|_| self.diag("array literal is too long"))?;
        self.emit(Op::Array(n));
        Ok(())
    }

    fn msg_expr(&mut self, m: &MsgExpr) -> R {
        match m.msgs.as_slice() {
            [only] => self.send(m.recv.as_ref(), only, false),
            msgs => {
                if let Some(r) = &m.recv {
                    self.expr(r)?;
                }
                for msg in msgs {
                    self.send_after(msg, m.recv.is_some(), false)?;
                }
                Ok(())
            }
        }
    }

    fn send(&mut self, recv: Option<&Expr>, m: &Msg, bare: bool) -> R {
        self.line = m.line;
        if self.try_inline(recv, m)? {
            return Ok(());
        }
        match recv {
            Some(r) => self.expr(r)?,
            None => drop(self.emit(Op::Nil)),
        }
        self.send_after(m, recv.is_some(), bare)
    }

    /// Sends `m` to the value already on the stack.
    fn send_after(&mut self, m: &Msg, has_recv: bool, bare: bool) -> R {
        self.line = m.line;
        let known = self.check_send(m, has_recv, bare);
        for a in &m.args {
            self.expr(a)?;
        }
        self.line = m.line;
        if !known {
            // The program is rejected, but keep the stack shape sane.
            self.emit(Op::Nil);
            return Ok(());
        }
        let argc = u8::try_from(m.args.len())
            .ok()
            .filter(|&n| usize::from(n) <= MAX_ARGS)
            .ok_or_else(|| {
                self.diag(format!(
                    "a message with {} arguments; the limit is {MAX_ARGS}",
                    m.args.len()
                ))
            })?;
        let id = self.sel(&m.sel)?;
        self.emit(Op::Send(id, argc));
        Ok(())
    }

    fn inline_body(&mut self, b: &BlockExpr) -> R {
        self.body(&b.body)
    }

    fn cond_jump(&mut self, when_true: bool) -> usize {
        self.emit(if when_true {
            Op::JumpIfTrue(TO_PATCH)
        } else {
            Op::JumpIfFalse(TO_PATCH)
        })
    }

    fn try_inline(&mut self, recv: Option<&Expr>, m: &Msg) -> R<bool> {
        let Some(r) = recv else { return Ok(false) };
        let line = m.line;
        match (m.sel.as_str(), m.args.as_slice()) {
            ("ifTrue:" | "ifFalse:", [a]) if literal_block(a).is_some() => {
                let Some(body) = literal_block(a) else {
                    return Ok(false);
                };
                self.expr(r)?;
                self.line = line;
                let skip = self.cond_jump(m.sel == "ifFalse:");
                self.inline_body(body)?;
                let end = self.emit(Op::Jump(TO_PATCH));
                self.patch(skip)?;
                self.emit(Op::Nil);
                self.patch(end)?;
            }
            ("ifTrue:ifFalse:" | "ifFalse:ifTrue:", [a, b])
                if literal_block(a).is_some() && literal_block(b).is_some() =>
            {
                let (Some(first), Some(second)) = (literal_block(a), literal_block(b)) else {
                    return Ok(false);
                };
                self.expr(r)?;
                self.line = line;
                let skip = self.cond_jump(m.sel == "ifFalse:ifTrue:");
                self.inline_body(first)?;
                let end = self.emit(Op::Jump(TO_PATCH));
                self.patch(skip)?;
                self.inline_body(second)?;
                self.patch(end)?;
            }
            ("and:" | "or:", [a]) if literal_block(a).is_some() => {
                let Some(body) = literal_block(a) else {
                    return Ok(false);
                };
                let is_and = m.sel == "and:";
                self.expr(r)?;
                self.line = line;
                let short = self.cond_jump(!is_and);
                self.inline_body(body)?;
                let end = self.emit(Op::Jump(TO_PATCH));
                self.patch(short)?;
                self.emit(if is_and { Op::False } else { Op::True });
                self.patch(end)?;
            }
            ("whileTrue:" | "whileFalse:", [a])
                if literal_block(r).is_some() && literal_block(a).is_some() =>
            {
                let (Some(cond), Some(body)) = (literal_block(r), literal_block(a)) else {
                    return Ok(false);
                };
                let start = self.here()?;
                self.inline_body(cond)?;
                self.line = line;
                let exit = self.cond_jump(m.sel == "whileFalse:");
                self.inline_body(body)?;
                self.emit(Op::Pop);
                self.emit(Op::Jump(start));
                self.patch(exit)?;
                self.emit(Op::Nil);
            }
            _ => return Ok(false),
        }
        Ok(true)
    }
}
