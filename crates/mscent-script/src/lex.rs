//! Lexer. Tokens are spans into the source, so a megabyte of hostile text
//! costs 16 bytes a token and no string allocations.

/// Largest source the lexer accepts, in bytes.
pub const MAX_SOURCE: usize = 1 << 20;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Tok {
    Ident,
    /// `word:`, the colon included in the span.
    Keyword,
    Num(f32),
    /// The span is the text between the quotes, with `''` not yet unescaped.
    Str,
    Bin(&'static str),
    Assign,
    Dot,
    Caret,
    Colon,
    Bar,
    LBracket,
    RBracket,
    LParen,
    RParen,
    LBrace,
    RBrace,
    Define,
    Eof,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Token {
    pub tok: Tok,
    pub line: u32,
    pub start: u32,
    pub end: u32,
}

#[derive(Clone, Debug)]
pub(crate) struct ParseError {
    pub line: u32,
    pub msg: String,
}

impl ParseError {
    pub(crate) fn new(line: u32, msg: impl Into<String>) -> Self {
        Self {
            line,
            msg: msg.into(),
        }
    }
}

struct Lexer<'a> {
    src: &'a str,
    bytes: &'a [u8],
    i: usize,
    line: u32,
    out: Vec<Token>,
}

pub(crate) fn lex(src: &str) -> Result<Vec<Token>, ParseError> {
    if src.len() > MAX_SOURCE {
        return Err(ParseError::new(
            1,
            format!("script is over the {MAX_SOURCE} byte limit"),
        ));
    }
    let mut lx = Lexer {
        src,
        bytes: src.as_bytes(),
        i: 0,
        line: 1,
        out: Vec::new(),
    };
    while let Some(c) = lx.peek(0) {
        lx.step(c)?;
    }
    lx.push(Tok::Eof, lx.i, lx.i)?;
    Ok(lx.out)
}

fn is_word_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

impl Lexer<'_> {
    fn peek(&self, ahead: usize) -> Option<u8> {
        self.bytes.get(self.i.checked_add(ahead)?).copied()
    }

    fn err<T>(&self, line: u32, msg: impl Into<String>) -> Result<T, ParseError> {
        Err(ParseError::new(line, msg))
    }

    fn push(&mut self, tok: Tok, start: usize, end: usize) -> Result<(), ParseError> {
        let (Ok(start), Ok(end)) = (u32::try_from(start), u32::try_from(end)) else {
            return self.err(self.line, "script is too large");
        };
        self.out.push(Token {
            tok,
            line: self.line,
            start,
            end,
        });
        Ok(())
    }

    fn text(&self, start: usize, end: usize) -> &str {
        self.src.get(start..end).unwrap_or("")
    }

    fn newline(&mut self) {
        self.line = self.line.saturating_add(1);
    }

    fn step(&mut self, c: u8) -> Result<(), ParseError> {
        match c {
            b'\n' => {
                self.newline();
                self.i += 1;
            }
            b' ' | b'\t' | b'\r' => self.i += 1,
            b'"' => self.comment()?,
            b'\'' => self.string()?,
            b'0'..=b'9' => self.number()?,
            b'a'..=b'z' | b'A'..=b'Z' | b'_' => self.word()?,
            _ => self.symbol(c)?,
        }
        Ok(())
    }

    fn comment(&mut self) -> Result<(), ParseError> {
        let start_line = self.line;
        self.i += 1;
        while let Some(c) = self.peek(0) {
            self.i += 1;
            match c {
                b'"' => return Ok(()),
                b'\n' => self.newline(),
                _ => {}
            }
        }
        self.err(start_line, "comment opened here is never closed")
    }

    fn string(&mut self) -> Result<(), ParseError> {
        let start_line = self.line;
        let open = self.i + 1;
        self.i = open;
        loop {
            match (self.peek(0), self.peek(1)) {
                (None, _) => return self.err(start_line, "string opened here is never closed"),
                (Some(b'\''), Some(b'\'')) => self.i += 2,
                (Some(b'\''), _) => break,
                (Some(c), _) => {
                    if c == b'\n' {
                        self.newline();
                    }
                    self.i += 1;
                }
            }
        }
        let line_after = self.line;
        self.line = start_line;
        let pushed = self.push(Tok::Str, open, self.i);
        self.line = line_after;
        self.i += 1;
        pushed
    }

    fn digits(&mut self) {
        while self.peek(0).is_some_and(|b| b.is_ascii_digit()) {
            self.i += 1;
        }
    }

    fn number(&mut self) -> Result<(), ParseError> {
        let start = self.i;
        self.digits();
        if self.peek(0) == Some(b'.') && self.peek(1).is_some_and(|b| b.is_ascii_digit()) {
            self.i += 1;
            self.digits();
        }
        let Ok(v) = self.text(start, self.i).parse::<f32>() else {
            return self.err(self.line, "bad number");
        };
        if !v.is_finite() {
            return self.err(self.line, "number is too large");
        }
        self.push(Tok::Num(v), start, self.i)
    }

    fn word(&mut self) -> Result<(), ParseError> {
        let start = self.i;
        while self.peek(0).is_some_and(is_word_byte) {
            self.i += 1;
        }
        if self.peek(0) == Some(b':') && self.peek(1) != Some(b'=') {
            self.i += 1;
            return self.push(Tok::Keyword, start, self.i);
        }
        let tok = if self.text(start, self.i) == "define" {
            Tok::Define
        } else {
            Tok::Ident
        };
        self.push(tok, start, self.i)
    }

    fn symbol(&mut self, c: u8) -> Result<(), ParseError> {
        let next = self.peek(1);
        let (tok, len) = match (c, next) {
            (b':', Some(b'=')) => (Tok::Assign, 2),
            (b'<', Some(b'=')) => (Tok::Bin("<="), 2),
            (b'>', Some(b'=')) => (Tok::Bin(">="), 2),
            (b'~', Some(b'=')) => (Tok::Bin("~="), 2),
            (b'+', _) => (Tok::Bin("+"), 1),
            (b'-', _) => (Tok::Bin("-"), 1),
            (b'*', _) => (Tok::Bin("*"), 1),
            (b'/', _) => (Tok::Bin("/"), 1),
            (b'<', _) => (Tok::Bin("<"), 1),
            (b'>', _) => (Tok::Bin(">"), 1),
            (b'=', _) => (Tok::Bin("="), 1),
            (b',', _) => (Tok::Bin(","), 1),
            (b':', _) => (Tok::Colon, 1),
            (b'.', _) => (Tok::Dot, 1),
            (b'^', _) => (Tok::Caret, 1),
            (b'|', _) => (Tok::Bar, 1),
            (b'[', _) => (Tok::LBracket, 1),
            (b']', _) => (Tok::RBracket, 1),
            (b'(', _) => (Tok::LParen, 1),
            (b')', _) => (Tok::RParen, 1),
            (b'{', _) => (Tok::LBrace, 1),
            (b'}', _) => (Tok::RBrace, 1),
            _ => {
                let ch = self
                    .src
                    .get(self.i..)
                    .and_then(|s| s.chars().next())
                    .unwrap_or('?');
                return self.err(self.line, format!("unexpected character {ch:?}"));
            }
        };
        self.push(tok, self.i, self.i + len)?;
        self.i += len;
        Ok(())
    }
}
