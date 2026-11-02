//! Recipes on disk: versioned text, diffable, and read as untrusted input.
//!
//! ```text
//! miniscent-recipe 1
//! size 128 128
//! gloss 25
//! # x y w op
//! op 0 0 1 noise freq_x=4 freq_y=4 seed=1
//! op 0 1 1 out-albedo
//! ```
//!
//! A value holds no spaces: text is percent-encoded and colours are `r,g,b` or
//! `r,g,b,a`. A `stroke radius hardness r,g,b,a x,y x,y...` line adds a brush
//! stroke to the `op` above it. Anything not given takes its table default, and
//! the writer leaves defaults out, so a table can grow without breaking files.

use std::sync::Arc;

use crate::error::TexError;
use crate::image::Size;
use crate::ops::{self, OpSpec, ParamKind, Params, Stroke, Value};

const HEADER: &str = "miniscent-recipe 1";

/// Largest recipe file, in bytes.
pub const MAX_FILE: usize = 4 * 1024 * 1024;
/// Longest line, in bytes: about enough for a stroke of 4096 points.
pub const MAX_LINE: usize = 64 * 1024;
/// Most blocks on a page.
pub const MAX_BLOCKS: usize = 256;
/// Most strokes one operator may hold.
pub const MAX_STROKES: usize = 4096;
/// Gloss when a recipe does not give one.
const DEFAULT_GLOSS: u8 = 40;
/// How far outside the tile a stroke may run. A path is wrapped when it is
/// drawn, so a little past the edge is normal and far past it is garbage.
const MAX_STROKE_COORD: f32 = 16.0;

/// One operator on the page. It reads the blocks above it (see [`crate::graph`]).
#[derive(Clone, Debug, PartialEq)]
pub struct Block {
    pub x: u16,
    pub y: u16,
    /// Columns it covers; at least 1.
    pub w: u16,
    pub spec: &'static OpSpec,
    pub params: Params,
}

/// A material's recipe. Its file name is its name.
#[derive(Clone, Debug, PartialEq)]
pub struct Recipe {
    pub size: Size,
    /// The gloss a new material starts from; `materials.txt` has the last word.
    pub gloss: u8,
    pub blocks: Vec<Block>,
}

fn parse_error(line: usize, why: impl Into<String>) -> TexError {
    TexError::Parse {
        line,
        why: why.into(),
    }
}

/// Decodes percent-encoded text; `None` for a bad escape or invalid UTF-8.
fn decode_text(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while let Some(&b) = bytes.get(i) {
        if b == b'%' {
            let hex = text.get(i + 1..i + 3)?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(b);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// Encodes text so it is one token on a line.
fn encode_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for b in text.bytes() {
        if b.is_ascii_graphic() && b != b'%' && b != b'#' && b != b'=' {
            out.push(char::from(b));
        } else {
            out.push_str(&format!("%{b:02x}"));
        }
    }
    out
}

fn parse_color(text: &str) -> Option<[f32; 4]> {
    let mut c = [0.0, 0.0, 0.0, 1.0];
    let mut count = 0;
    for part in text.split(',') {
        *c.get_mut(count)? = part.parse().ok()?;
        count += 1;
    }
    (count == 3 || count == 4).then_some(c)
}

fn format_color(c: &[f32; 4]) -> String {
    if c[3] == 1.0 {
        format!("{},{},{}", c[0], c[1], c[2])
    } else {
        format!("{},{},{},{}", c[0], c[1], c[2], c[3])
    }
}

fn parse_value(kind: &ParamKind, text: &str) -> Option<Value> {
    Some(match *kind {
        ParamKind::Float { .. } => Value::Float(text.parse().ok()?),
        ParamKind::Int { .. } => Value::Int(text.parse().ok()?),
        ParamKind::Choice { names, .. } => {
            Value::Choice(u8::try_from(names.iter().position(|n| *n == text)?).ok()?)
        }
        ParamKind::Color { .. } => Value::Color(parse_color(text)?),
        ParamKind::Text { .. } => Value::Text(decode_text(text)?),
        ParamKind::Strokes => return None,
    })
}

fn format_value(kind: &ParamKind, value: &Value) -> Option<String> {
    Some(match (kind, value) {
        (_, Value::Float(v)) => v.to_string(),
        (_, Value::Int(v)) => v.to_string(),
        (ParamKind::Choice { names, .. }, Value::Choice(v)) => {
            (*names.get(usize::from(*v))?).to_string()
        }
        (_, Value::Color(c)) => format_color(c),
        (_, Value::Text(t)) => encode_text(t),
        _ => return None,
    })
}

fn parse_u16(text: Option<&str>, line: usize, what: &str) -> Result<u16, TexError> {
    text.and_then(|t| t.parse().ok())
        .ok_or_else(|| parse_error(line, format!("bad {what}")))
}

fn parse_block<'a>(
    mut tokens: impl Iterator<Item = &'a str>,
    line: usize,
) -> Result<Block, TexError> {
    let x = parse_u16(tokens.next(), line, "x")?;
    let y = parse_u16(tokens.next(), line, "y")?;
    let w = parse_u16(tokens.next(), line, "width")?;
    let name = tokens.next().unwrap_or_default();
    let spec = ops::find(name).ok_or_else(|| parse_error(line, "unknown operator"))?;
    let mut params = Params::defaults(spec);
    for token in tokens {
        let (key, text) = token
            .split_once('=')
            .ok_or_else(|| parse_error(line, format!("expected key=value: {token}")))?;
        let param = spec
            .params
            .iter()
            .find(|p| p.name == key)
            .ok_or_else(|| parse_error(line, format!("unknown parameter {key}")))?;
        let value = parse_value(&param.kind, text)
            .ok_or_else(|| parse_error(line, format!("bad value for {key}")))?;
        params
            .set(spec, key, value)
            .map_err(|e| parse_error(line, e.to_string()))?;
    }
    Ok(Block {
        x,
        y,
        w,
        spec,
        params,
    })
}

fn unit(text: Option<&str>) -> Option<f32> {
    let v: f32 = text?.parse().ok()?;
    (0.0..=1.0).contains(&v).then_some(v)
}

fn parse_stroke<'a>(mut tokens: impl Iterator<Item = &'a str>) -> Option<Stroke> {
    let radius = unit(tokens.next())?;
    let hardness = unit(tokens.next())?;
    let color = parse_color(tokens.next()?)?;
    if !color.iter().all(|c| (0.0..=1.0).contains(c)) {
        return None;
    }
    let mut points = Vec::new();
    for token in tokens {
        let (x, y) = token.split_once(',')?;
        let (x, y): (f32, f32) = (x.parse().ok()?, y.parse().ok()?);
        let fits = |v: f32| v.abs() <= MAX_STROKE_COORD;
        if !fits(x) || !fits(y) {
            return None;
        }
        points.push([x, y]);
    }
    Some(Stroke {
        radius,
        hardness,
        color,
        points,
    })
}

/// Adds `stroke` to `block`'s strokes parameter.
fn add_stroke(block: &mut Block, stroke: Stroke, line: usize) -> Result<(), TexError> {
    let spec = block.spec;
    let param = spec
        .params
        .iter()
        .position(|p| matches!(p.kind, ParamKind::Strokes))
        .and_then(|i| spec.params.get(i))
        .ok_or_else(|| parse_error(line, "this operator takes no strokes"))?;
    let index = spec.params.iter().position(|p| p.name == param.name);
    let existing = index.and_then(|i| block.params.values().get(i));
    let mut strokes = match existing {
        Some(Value::Strokes(s)) => s.as_ref().clone(),
        _ => Vec::new(),
    };
    if strokes.len() >= MAX_STROKES {
        return Err(parse_error(line, "too many strokes"));
    }
    strokes.push(stroke);
    block
        .params
        .set(spec, param.name, Value::Strokes(Arc::new(strokes)))
}

/// Reads a recipe. Whether its blocks connect is [`crate::graph::compile`]'s
/// question.
pub fn read(bytes: &[u8]) -> Result<Recipe, TexError> {
    if bytes.len() > MAX_FILE {
        return Err(parse_error(0, "file too large"));
    }
    let text = core::str::from_utf8(bytes).map_err(|_| parse_error(0, "not UTF-8"))?;
    let mut lines = text.lines().enumerate().map(|(i, l)| (i + 1, l.trim()));
    if lines.next().map(|l| l.1) != Some(HEADER) {
        return Err(parse_error(1, "not a version 1 recipe"));
    }
    let (mut size, mut gloss, mut blocks) = (None, DEFAULT_GLOSS, Vec::<Block>::new());
    for (line, content) in lines {
        if content.len() > MAX_LINE {
            return Err(parse_error(line, "line too long"));
        }
        if content.is_empty() || content.starts_with('#') {
            continue;
        }
        let mut tokens = content.split_ascii_whitespace();
        match tokens.next() {
            Some("size") => {
                let w = parse_u16(tokens.next(), line, "width")?;
                let h = parse_u16(tokens.next(), line, "height")?;
                size = Some(
                    Size::new(u32::from(w), u32::from(h))
                        .map_err(|e| parse_error(line, e.to_string()))?,
                );
            }
            Some("gloss") => {
                gloss = tokens
                    .next()
                    .and_then(|t| t.parse().ok())
                    .ok_or_else(|| parse_error(line, "gloss is 0 to 255"))?;
            }
            Some("op") => {
                if blocks.len() >= MAX_BLOCKS {
                    return Err(parse_error(line, "too many blocks"));
                }
                blocks.push(parse_block(tokens, line)?);
            }
            Some("stroke") => {
                let stroke = parse_stroke(tokens).ok_or_else(|| parse_error(line, "bad stroke"))?;
                let block = blocks
                    .last_mut()
                    .ok_or_else(|| parse_error(line, "stroke before any op"))?;
                add_stroke(block, stroke, line)?;
            }
            Some(other) => return Err(parse_error(line, format!("unknown line: {other}"))),
            None => {}
        }
    }
    Ok(Recipe {
        size: size.ok_or_else(|| parse_error(0, "no size line"))?,
        gloss,
        blocks,
    })
}

fn write_block(out: &mut String, block: &Block) {
    let spec = block.spec;
    out.push_str(&format!(
        "op {} {} {} {}",
        block.x, block.y, block.w, spec.name
    ));
    let mut strokes: &[Stroke] = &[];
    for (param, value) in spec.params.iter().zip(block.params.values()) {
        if let Value::Strokes(s) = value {
            strokes = s;
        } else if *value != param.kind.default_value()
            && let Some(text) = format_value(&param.kind, value)
        {
            out.push_str(&format!(" {}={text}", param.name));
        }
    }
    out.push('\n');
    for s in strokes {
        out.push_str(&format!(
            "stroke {} {} {},{},{},{}",
            s.radius, s.hardness, s.color[0], s.color[1], s.color[2], s.color[3]
        ));
        for p in &s.points {
            out.push_str(&format!(" {},{}", p[0], p[1]));
        }
        out.push('\n');
    }
}

/// Writes a recipe that [`read`] gives back unchanged.
pub fn write(recipe: &Recipe) -> String {
    let mut out = format!(
        "{HEADER}\nsize {} {}\ngloss {}\n",
        recipe.size.width(),
        recipe.size.height(),
        recipe.gloss
    );
    for block in &recipe.blocks {
        write_block(&mut out, block);
    }
    out
}

#[cfg(test)]
#[path = "recipe_tests.rs"]
mod recipe_tests;
