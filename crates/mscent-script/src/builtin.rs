//! The language's own messages: arithmetic, comparison, text, arrays.

use crate::heap::{Heap, MAX_ARRAY_LEN, MAX_STR_LEN, Obj, Value};

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Builtin {
    Add,
    Sub,
    Mul,
    Div,
    Lt,
    Gt,
    Le,
    Ge,
    Eq,
    Ne,
    Concat,
    Not,
    IsNil,
    Size,
    At,
    AtPut,
    Append,
}

pub(crate) const BUILTINS: &[(&str, Builtin)] = &[
    ("+", Builtin::Add),
    ("-", Builtin::Sub),
    ("*", Builtin::Mul),
    ("/", Builtin::Div),
    ("<", Builtin::Lt),
    (">", Builtin::Gt),
    ("<=", Builtin::Le),
    (">=", Builtin::Ge),
    ("=", Builtin::Eq),
    ("~=", Builtin::Ne),
    (",", Builtin::Concat),
    ("not", Builtin::Not),
    ("isNil", Builtin::IsNil),
    ("size", Builtin::Size),
    ("at:", Builtin::At),
    ("at:put:", Builtin::AtPut),
    ("add:", Builtin::Append),
];

/// Selectors the VM handles itself: blocks, tasks and time.
pub(crate) const VM_SELECTORS: &[&str] = &["value", "value:", "value:value:", "fork:", "wait:"];

/// Selectors the compiler turns into jumps when written with literal blocks.
pub(crate) const INLINE_SELECTORS: &[&str] = &[
    "ifTrue:",
    "ifFalse:",
    "ifTrue:ifFalse:",
    "ifFalse:ifTrue:",
    "whileTrue:",
    "whileFalse:",
    "and:",
    "or:",
];

/// Every selector the language itself provides, for `--script-api`.
pub fn language_selectors() -> Vec<&'static str> {
    let builtin = BUILTINS.iter().map(|b| b.0);
    builtin
        .chain(VM_SELECTORS.iter().copied())
        .chain(INLINE_SELECTORS.iter().copied())
        .collect()
}

fn name(b: Builtin) -> &'static str {
    BUILTINS.iter().find(|x| x.1 == b).map_or("?", |x| x.0)
}

fn finite(v: f32) -> Result<Value, String> {
    if v.is_finite() {
        Ok(Value::Num(v))
    } else {
        Err("arithmetic gave a number that is not finite".to_string())
    }
}

pub(crate) fn apply(
    h: &mut Heap,
    b: Builtin,
    recv: Value,
    args: &[Value],
) -> Result<Value, String> {
    use Builtin as B;
    match (b, recv, args) {
        (B::Add, Value::Num(x), [Value::Num(y)]) => finite(x + y),
        (B::Sub, Value::Num(x), [Value::Num(y)]) => finite(x - y),
        (B::Mul, Value::Num(x), [Value::Num(y)]) => finite(x * y),
        (B::Div, Value::Num(x), [Value::Num(y)]) => finite(x / y),
        (B::Lt, Value::Num(x), [Value::Num(y)]) => Ok(Value::Bool(x < *y)),
        (B::Gt, Value::Num(x), [Value::Num(y)]) => Ok(Value::Bool(x > *y)),
        (B::Le, Value::Num(x), [Value::Num(y)]) => Ok(Value::Bool(x <= *y)),
        (B::Ge, Value::Num(x), [Value::Num(y)]) => Ok(Value::Bool(x >= *y)),
        (B::Eq, x, [y]) => Ok(Value::Bool(equal(h, x, *y))),
        (B::Ne, x, [y]) => Ok(Value::Bool(!equal(h, x, *y))),
        (B::Concat, x, [y]) => concat(h, x, *y),
        (B::Not, Value::Bool(x), []) => Ok(Value::Bool(!x)),
        (B::IsNil, x, []) => Ok(Value::Bool(x == Value::Nil)),
        (B::Size, x, []) => size(h, x),
        (B::At, Value::Ref(_), [Value::Num(n)]) => at(h, recv, *n),
        (B::AtPut, Value::Ref(r), [Value::Num(n), x]) => {
            let len = h.array(recv).map_or(0, <[Value]>::len);
            let i = index(*n, len)?;
            let slot = h.array_mut(r).and_then(|a| a.get_mut(i));
            let slot = slot.ok_or("at:put: sent to something that is not an array")?;
            *slot = *x;
            Ok(*x)
        }
        (B::Append, Value::Ref(r), [x]) => h.array_push(r, *x).map(|()| *x),
        _ => Err(format!(
            "{} does not understand {}",
            h.describe(recv),
            name(b)
        )),
    }
}

fn concat(h: &mut Heap, x: Value, y: Value) -> Result<Value, String> {
    let (a, b) = (h.text(x), h.text(y));
    if a.len().saturating_add(b.len()) > MAX_STR_LEN {
        return Err(format!("string longer than {MAX_STR_LEN} bytes"));
    }
    h.new_str(a + &b)
}

fn size(h: &Heap, x: Value) -> Result<Value, String> {
    let len = match x {
        Value::Ref(r) => match h.get(r) {
            Some(Obj::Array(v)) => v.len(),
            Some(Obj::Str(s)) => s.len(),
            _ => return Err("size sent to something with no size".into()),
        },
        _ => return Err(format!("{} does not understand size", h.describe(x))),
    };
    // Both are capped far below 2^24, so the f32 is exact.
    Ok(Value::Num(len as f32))
}

fn at(h: &Heap, recv: Value, n: f32) -> Result<Value, String> {
    let Some(items) = h.array(recv) else {
        return Err("at: sent to something that is not an array".into());
    };
    let i = index(n, items.len())?;
    items
        .get(i)
        .copied()
        .ok_or_else(|| "index out of range".to_string())
}

/// 1-based index to 0-based, rejecting fractions, zero and anything past the end.
fn index(n: f32, len: usize) -> Result<usize, String> {
    if n >= 1.0 && n <= MAX_ARRAY_LEN as f32 && n.fract() == 0.0 && (n as usize) <= len {
        Ok(n as usize - 1)
    } else {
        Err(format!("index {n} is outside 1..{len}"))
    }
}

fn equal(h: &Heap, a: Value, b: Value) -> bool {
    match (h.str(a), h.str(b)) {
        (Some(x), Some(y)) => x == y,
        _ => a == b,
    }
}
