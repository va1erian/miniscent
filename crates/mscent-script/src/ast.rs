//! The syntax tree. Unary and binary message chains are flat vectors, not
//! nested boxes: `1 + 1 + 1 + ...` a hundred thousand times stays one node
//! deep, so neither the compiler nor `Drop` recurses along it. Real nesting
//! (parentheses, blocks, arrays) is capped by the parser's depth limit.

#[derive(Debug)]
pub(crate) enum Expr {
    Num(f32, u32),
    Str(String, u32),
    Var(String, u32),
    Assign(String, Box<Expr>, u32),
    Block(Box<BlockExpr>),
    Array(Vec<Expr>, u32),
    Return(Box<Expr>, u32),
    Msg(Box<MsgExpr>),
}

#[derive(Debug)]
pub(crate) struct BlockExpr {
    pub params: Vec<String>,
    pub body: Vec<Expr>,
    pub line: u32,
}

/// A receiver followed by messages, each sent to the previous result. A
/// keyword message is always alone, and has no receiver when written bare.
#[derive(Debug)]
pub(crate) struct MsgExpr {
    pub recv: Option<Expr>,
    pub msgs: Vec<Msg>,
}

#[derive(Debug)]
pub(crate) struct Msg {
    pub sel: String,
    pub args: Vec<Expr>,
    pub line: u32,
    /// For a keyword message, the line of each `word:` part, so a missing
    /// period can name the line it belongs on. Empty otherwise.
    pub part_lines: Vec<u32>,
}

#[derive(Debug)]
pub(crate) struct Define {
    pub recv: Option<String>,
    pub sel: String,
    pub params: Vec<String>,
    pub body: Vec<Expr>,
    pub line: u32,
}

#[derive(Debug, Default)]
pub(crate) struct Program {
    pub defines: Vec<Define>,
    pub top: Vec<Expr>,
}
