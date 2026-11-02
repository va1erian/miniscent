//! Recursive descent parser. Precedence is unary, then binary (strictly left
//! to right), then keyword. Only real nesting recurses, and it is capped.

use crate::ast::{BlockExpr, Define, Expr, Msg, MsgExpr, Program};
use crate::lex::{ParseError, Tok, Token, lex};

/// Deepest nesting of parentheses, blocks, array items and assignments. Each
/// level costs a handful of Rust stack frames in the parser and again in the
/// compiler, so this keeps both far inside a 2 MiB thread stack even in an
/// unoptimized build.
const MAX_DEPTH: u32 = 64;

type Res<T> = Result<T, ParseError>;

pub(crate) fn parse(src: &str) -> Res<Program> {
    let mut p = Parser {
        src,
        toks: lex(src)?,
        i: 0,
        depth: 0,
    };
    p.program()
}

struct Parser<'a> {
    src: &'a str,
    toks: Vec<Token>,
    i: usize,
    depth: u32,
}

fn unescape(raw: &str) -> String {
    raw.replace("''", "'")
}

fn check_name(name: &str, line: u32, what: &str) -> Res<()> {
    if matches!(name, "nil" | "true" | "false") {
        return Err(ParseError::new(
            line,
            format!("'{name}' is a value, so it cannot be a {what} name"),
        ));
    }
    Ok(())
}

fn check_params(names: &[&str], line: u32) -> Res<()> {
    for (i, n) in names.iter().enumerate() {
        check_name(n, line, "parameter")?;
        if names.iter().take(i).any(|m| m == n) {
            return Err(ParseError::new(
                line,
                format!("parameter '{n}' appears twice"),
            ));
        }
    }
    Ok(())
}

impl Parser<'_> {
    fn cur(&self) -> Token {
        let fallback = Token {
            tok: Tok::Eof,
            line: 1,
            start: 0,
            end: 0,
        };
        self.toks
            .get(self.i)
            .or(self.toks.last())
            .copied()
            .unwrap_or(fallback)
    }

    fn tok(&self) -> Tok {
        self.cur().tok
    }

    fn line(&self) -> u32 {
        self.cur().line
    }

    fn peek_at(&self, ahead: usize) -> Tok {
        self.toks
            .get(self.i.saturating_add(ahead))
            .map_or(Tok::Eof, |t| t.tok)
    }

    fn advance(&mut self) -> Token {
        let t = self.cur();
        if t.tok != Tok::Eof {
            self.i += 1;
        }
        t
    }

    fn text(&self, t: Token) -> &str {
        self.src.get(t.start as usize..t.end as usize).unwrap_or("")
    }

    fn found(&self, t: Token) -> String {
        match t.tok {
            Tok::Ident | Tok::Keyword => format!("'{}'", crate::error::shorten(self.text(t))),
            Tok::Num(_) => format!("the number {}", crate::error::shorten(self.text(t))),
            Tok::Str => "a string".into(),
            Tok::Bin(op) => format!("'{op}'"),
            Tok::Assign => "':='".into(),
            Tok::Dot => "'.'".into(),
            Tok::Caret => "'^'".into(),
            Tok::Colon => "':'".into(),
            Tok::Bar => "'|'".into(),
            Tok::LBracket => "'['".into(),
            Tok::RBracket => "']'".into(),
            Tok::LParen => "'('".into(),
            Tok::RParen => "')'".into(),
            Tok::LBrace => "'{'".into(),
            Tok::RBrace => "'}'".into(),
            Tok::Define => "'define'".into(),
            Tok::Eof => "the end of the file".into(),
        }
    }

    fn err<T>(&self, msg: impl Into<String>) -> Res<T> {
        Err(ParseError::new(self.line(), msg))
    }

    fn expect(&mut self, tok: Tok, what: &str) -> Res<Token> {
        if self.tok() == tok {
            return Ok(self.advance());
        }
        self.err(format!("expected {what}, found {}", self.found(self.cur())))
    }

    fn eat_dots(&mut self) {
        while self.tok() == Tok::Dot {
            self.advance();
        }
    }

    fn enter(&mut self, what: &str) -> Res<()> {
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            return self.err(format!("{what} nested more than {MAX_DEPTH} deep"));
        }
        Ok(())
    }

    fn leave(&mut self) {
        self.depth = self.depth.saturating_sub(1);
    }

    fn program(&mut self) -> Res<Program> {
        let mut prog = Program::default();
        loop {
            self.eat_dots();
            match self.tok() {
                Tok::Eof => return Ok(prog),
                Tok::Define => prog.defines.push(self.define()?),
                _ => {
                    prog.top.push(self.statement()?);
                    if !matches!(self.tok(), Tok::Dot | Tok::Eof) {
                        return self.missing_period();
                    }
                }
            }
        }
    }

    /// After a statement comes a period, the end of its block or the end of
    /// the file. Anything else usually means a period was forgotten.
    fn missing_period<T>(&self) -> Res<T> {
        let prev = self
            .i
            .checked_sub(1)
            .and_then(|i| self.toks.get(i))
            .map_or(0, |t| t.line);
        let hint = if prev != 0 && prev < self.line() {
            format!(" (a '.' is probably missing at the end of line {prev})")
        } else {
            String::new()
        };
        self.err(format!(
            "expected '.' between statements, found {}{hint}",
            self.found(self.cur())
        ))
    }

    fn statements(&mut self, open_line: u32) -> Res<Vec<Expr>> {
        let mut body = Vec::new();
        loop {
            self.eat_dots();
            match self.tok() {
                Tok::RBracket => return Ok(body),
                Tok::Eof => {
                    return Err(ParseError::new(
                        open_line,
                        "this block is never closed: a ']' is missing before the end of the file",
                    ));
                }
                _ => {}
            }
            body.push(self.statement()?);
            if !matches!(self.tok(), Tok::Dot | Tok::RBracket | Tok::Eof) {
                return self.missing_period();
            }
        }
    }

    /// `define autoDoor: name key: k [ ... ]`, `define arr do: blk [ ... ]`
    /// (the first word names the receiver) or `define thing frob [ ... ]`.
    fn define(&mut self) -> Res<Define> {
        let line = self.advance().line;
        let (mut recv, mut sel) = (None, String::new());
        if self.tok() == Tok::Ident {
            let first = self.advance();
            let first = self.text(first).to_string();
            if self.tok() == Tok::Ident {
                let u = self.advance();
                sel = self.text(u).to_string();
                recv = Some(first);
            } else if self.tok() == Tok::Keyword {
                recv = Some(first);
            } else {
                sel = first;
            }
        }
        let mut params = Vec::new();
        while self.tok() == Tok::Keyword {
            let k = self.advance();
            sel.push_str(self.text(k));
            let p = self.expect(Tok::Ident, "a parameter name after the keyword")?;
            params.push(self.text(p).to_string());
        }
        if sel.is_empty() {
            return self.err("expected a name after define");
        }
        let names: Vec<&str> = recv
            .iter()
            .chain(params.iter())
            .map(String::as_str)
            .collect();
        check_params(&names, line)?;
        let open = self.expect(Tok::LBracket, "'[' to start the body")?;
        let body = self.statements(open.line)?;
        self.expect(Tok::RBracket, "']'")?;
        Ok(Define {
            recv,
            sel,
            params,
            body,
            line,
        })
    }

    fn statement(&mut self) -> Res<Expr> {
        let line = self.line();
        if self.tok() == Tok::Caret {
            self.advance();
            return Ok(Expr::Return(Box::new(self.expr()?), line));
        }
        self.expr()
    }

    fn expr(&mut self) -> Res<Expr> {
        self.enter("expressions")?;
        let r = self.expr_inner();
        self.leave();
        r
    }

    fn expr_inner(&mut self) -> Res<Expr> {
        if self.tok() == Tok::Ident && self.peek_at(1) == Tok::Assign {
            let name = self.advance();
            let line = name.line;
            let name = self.text(name).to_string();
            check_name(&name, line, "variable")?;
            self.advance();
            return Ok(Expr::Assign(name, Box::new(self.expr()?), line));
        }
        if self.tok() == Tok::Keyword {
            return self.keyword_message(None);
        }
        let recv = self.binary()?;
        if self.tok() == Tok::Keyword {
            return self.keyword_message(Some(recv));
        }
        Ok(recv)
    }

    fn keyword_message(&mut self, recv: Option<Expr>) -> Res<Expr> {
        let line = self.line();
        let (mut sel, mut args, mut part_lines) = (String::new(), Vec::new(), Vec::new());
        while self.tok() == Tok::Keyword {
            let k = self.advance();
            sel.push_str(self.text(k));
            part_lines.push(k.line);
            args.push(self.binary()?);
        }
        let msg = Msg {
            sel,
            args,
            line,
            part_lines,
        };
        Ok(Expr::Msg(Box::new(MsgExpr {
            recv,
            msgs: vec![msg],
        })))
    }

    /// `a + b * c` is `(a + b) * c`. The links are a flat list, not a tree.
    fn binary(&mut self) -> Res<Expr> {
        let head = self.unary()?;
        let mut msgs = Vec::new();
        while let Tok::Bin(op) = self.tok() {
            let line = self.advance().line;
            let arg = self.unary()?;
            msgs.push(Msg {
                sel: op.to_string(),
                args: vec![arg],
                line,
                part_lines: Vec::new(),
            });
        }
        Ok(chain(head, msgs))
    }

    fn unary(&mut self) -> Res<Expr> {
        let head = self.primary()?;
        let mut msgs = Vec::new();
        // `word :=` starts the next statement, so a forgotten period stops the chain.
        while self.tok() == Tok::Ident && self.peek_at(1) != Tok::Assign {
            let t = self.advance();
            let sel = self.text(t).to_string();
            msgs.push(Msg {
                sel,
                args: Vec::new(),
                line: t.line,
                part_lines: Vec::new(),
            });
        }
        Ok(chain(head, msgs))
    }

    fn primary(&mut self) -> Res<Expr> {
        let t = self.cur();
        let line = t.line;
        match t.tok {
            Tok::Ident => {
                self.advance();
                Ok(Expr::Var(self.text(t).to_string(), line))
            }
            Tok::Num(v) => {
                self.advance();
                Ok(Expr::Num(v, line))
            }
            Tok::Str => {
                self.advance();
                Ok(Expr::Str(unescape(self.text(t)), line))
            }
            Tok::Bin("-") => {
                self.advance();
                match self.tok() {
                    Tok::Num(v) => {
                        self.advance();
                        Ok(Expr::Num(-v, line))
                    }
                    _ => self.err("expected a number after '-'"),
                }
            }
            Tok::LParen => {
                self.advance();
                let e = self.expr()?;
                self.expect(Tok::RParen, "')'")?;
                Ok(e)
            }
            Tok::LBracket => self.block(),
            Tok::LBrace => self.array(),
            _ => self.err(format!("expected a value, found {}", self.found(t))),
        }
    }

    fn block(&mut self) -> Res<Expr> {
        let open = self.advance();
        self.enter("blocks")?;
        let mut params = Vec::new();
        while self.tok() == Tok::Colon {
            self.advance();
            let p = self.expect(Tok::Ident, "a parameter name after ':'")?;
            params.push(self.text(p).to_string());
        }
        if !params.is_empty() {
            self.expect(Tok::Bar, "'|' after the block parameters")?;
        }
        let names: Vec<&str> = params.iter().map(String::as_str).collect();
        check_params(&names, open.line)?;
        let body = self.statements(open.line)?;
        self.expect(Tok::RBracket, "']'")?;
        self.leave();
        Ok(Expr::Block(Box::new(BlockExpr {
            params,
            body,
            line: open.line,
        })))
    }

    fn array(&mut self) -> Res<Expr> {
        let open = self.advance();
        let mut items = Vec::new();
        loop {
            self.eat_dots();
            if self.tok() == Tok::RBrace {
                self.advance();
                return Ok(Expr::Array(items, open.line));
            }
            items.push(self.expr()?);
            if !matches!(self.tok(), Tok::Dot | Tok::RBrace) {
                return self.err(format!(
                    "expected '.' between array items, found {}",
                    self.found(self.cur())
                ));
            }
        }
    }
}

fn chain(head: Expr, msgs: Vec<Msg>) -> Expr {
    if msgs.is_empty() {
        head
    } else {
        Expr::Msg(Box::new(MsgExpr {
            recv: Some(head),
            msgs,
        }))
    }
}
