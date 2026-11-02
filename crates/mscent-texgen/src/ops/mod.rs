//! The operators, and the table that describes them. An operator is one
//! [`OpSpec`]: its name, its inputs, its parameters and a kernel. The recipe
//! reader, the editor's parameter panel and the tests all work from the table,
//! so adding an operator takes one entry and one function.
//!
//! **Tiling.** Every kernel must give a tile that repeats. `tests.rs` proves it
//! for each operator: running it with its window moved by half a tile, on inputs
//! moved the same way, must give the same picture moved by half a tile. A pattern
//! that does not repeat, or a filter that reads past the edge without wrapping,
//! fails that. So generators draw through [`Frame::fill`], warps read through
//! [`Frame::sample`], and filters read through [`Image::at`].
//!
//! **Parameters are normalized** where they are lengths: a blur radius is a
//! fraction of the tile, so a recipe looks the same at every size.

use std::sync::Arc;

use crate::error::TexError;
use crate::image::{Frame, Image};

mod combine;
mod filter;
mod flow;
mod pattern;

/// Longest text parameter, in bytes.
pub const MAX_TEXT: usize = 64;

/// What a parameter holds, with its range and default.
#[derive(Clone, Copy, Debug)]
pub enum ParamKind {
    Float {
        min: f32,
        max: f32,
        default: f32,
    },
    Int {
        min: i32,
        max: i32,
        default: i32,
    },
    Choice {
        names: &'static [&'static str],
        default: u8,
    },
    Color {
        default: [f32; 4],
    },
    Text {
        default: &'static str,
    },
    /// Brush strokes, edited on the preview, not in the panel.
    Strokes,
}

/// One parameter of an operator.
#[derive(Clone, Copy, Debug)]
pub struct ParamSpec {
    pub name: &'static str,
    pub kind: ParamKind,
}

/// A brush stroke: a path in normalized coordinates, drawn with a round brush.
#[derive(Clone, Debug, PartialEq)]
pub struct Stroke {
    /// Fraction of the tile's width.
    pub radius: f32,
    /// `1.0` is a hard edge, `0.0` fades from the centre.
    pub hardness: f32,
    pub color: [f32; 4],
    pub points: Vec<[f32; 2]>,
}

/// A parameter's value.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Float(f32),
    Int(i32),
    Choice(u8),
    Color([f32; 4]),
    Text(String),
    Strokes(Arc<Vec<Stroke>>),
}

impl ParamKind {
    pub fn default_value(&self) -> Value {
        match *self {
            Self::Float { default, .. } => Value::Float(default),
            Self::Int { default, .. } => Value::Int(default),
            Self::Choice { default, .. } => Value::Choice(default),
            Self::Color { default } => Value::Color(default),
            Self::Text { default } => Value::Text(default.to_string()),
            Self::Strokes => Value::Strokes(Arc::default()),
        }
    }

    /// Whether `value` is of this kind and in range. NaN is never in range.
    pub fn accepts(&self, value: &Value) -> bool {
        match (*self, value) {
            (Self::Float { min, max, .. }, Value::Float(v)) => (min..=max).contains(v),
            (Self::Int { min, max, .. }, Value::Int(v)) => (min..=max).contains(v),
            (Self::Choice { names, .. }, Value::Choice(v)) => usize::from(*v) < names.len(),
            (Self::Color { .. }, Value::Color(c)) => c.iter().all(|v| (0.0..=1.0).contains(v)),
            (Self::Text { .. }, Value::Text(t)) => t.len() <= MAX_TEXT,
            (Self::Strokes, Value::Strokes(_)) => true,
            _ => false,
        }
    }
}

/// What the graph does with an operator, beyond running its kernel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    /// Makes an image from its parameters and inputs.
    Kernel,
    /// Passes its input on, and names it for [`Role::Load`].
    Store,
    /// Reads what a [`Role::Store`] of the same name holds.
    Load,
    /// Passes its input on, as one of the material's maps.
    Output(Channel),
}

/// A map of a finished material.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Channel {
    Albedo,
    Height,
    Specular,
}

/// Inputs a kernel may take, read from the blocks above it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Inputs {
    pub min: u8,
    pub max: u8,
}

/// An operator.
#[derive(Debug)]
pub struct OpSpec {
    /// Its name in recipes; unique.
    pub name: &'static str,
    /// Its menu in the editor.
    pub group: &'static str,
    pub inputs: Inputs,
    pub params: &'static [ParamSpec],
    pub role: Role,
    pub kernel: fn(&Ctx) -> Result<Image, TexError>,
}

impl PartialEq for OpSpec {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
    }
}

/// An operator's parameters, one value per [`OpSpec::params`] entry, in order.
#[derive(Clone, Debug, PartialEq)]
pub struct Params {
    values: Vec<Value>,
}

impl Params {
    pub fn defaults(spec: &OpSpec) -> Self {
        Self {
            values: spec.params.iter().map(|p| p.kind.default_value()).collect(),
        }
    }

    pub fn values(&self) -> &[Value] {
        &self.values
    }

    /// Sets the named parameter; out of range is an error, not a clamp.
    pub fn set(&mut self, spec: &OpSpec, name: &str, value: Value) -> Result<(), TexError> {
        let index = spec
            .params
            .iter()
            .position(|p| p.name == name)
            .ok_or_else(|| TexError::UnknownParam(name.to_string()))?;
        let slot = self
            .values
            .get_mut(index)
            .ok_or_else(|| TexError::UnknownParam(name.to_string()))?;
        if !spec.params[index].kind.accepts(&value) {
            return Err(TexError::BadParam(name.to_string()));
        }
        *slot = value;
        Ok(())
    }
}

/// What a kernel gets. The accessors read a parameter by name; a name the
/// operator's table does not have is a bug in the kernel, caught by the tests in
/// debug builds and reading as zero otherwise.
pub struct Ctx<'a> {
    pub frame: Frame,
    pub spec: &'static OpSpec,
    pub params: &'a Params,
    pub inputs: &'a [&'a Image],
}

impl Ctx<'_> {
    fn value(&self, name: &str) -> Option<&Value> {
        let index = self.spec.params.iter().position(|p| p.name == name);
        debug_assert!(index.is_some(), "{}: no parameter {name}", self.spec.name);
        self.params.values.get(index?)
    }

    pub fn float(&self, name: &str) -> f32 {
        match self.value(name) {
            Some(Value::Float(v)) => *v,
            _ => 0.0,
        }
    }

    pub fn int(&self, name: &str) -> i32 {
        match self.value(name) {
            Some(Value::Int(v)) => *v,
            _ => 0,
        }
    }

    pub fn choice(&self, name: &str) -> u8 {
        match self.value(name) {
            Some(Value::Choice(v)) => *v,
            _ => 0,
        }
    }

    pub fn color(&self, name: &str) -> [f32; 4] {
        match self.value(name) {
            Some(Value::Color(v)) => *v,
            _ => [0.0; 4],
        }
    }

    pub fn strokes(&self, name: &str) -> &[Stroke] {
        match self.value(name) {
            Some(Value::Strokes(v)) => v,
            _ => &[],
        }
    }

    /// Input `index`; the eval checks the count against the spec's minimum, so
    /// reading below it cannot fail.
    pub fn input(&self, index: usize) -> Result<&Image, TexError> {
        self.inputs
            .get(index)
            .copied()
            .ok_or(TexError::BadInputs(self.spec.name))
    }
}

/// Every operator, in menu order.
pub fn all() -> impl Iterator<Item = &'static OpSpec> {
    [pattern::OPS, combine::OPS, filter::OPS, flow::OPS]
        .into_iter()
        .flatten()
}

/// The operator of a name.
pub fn find(name: &str) -> Option<&'static OpSpec> {
    all().find(|op| op.name == name)
}

/// Runs one operator. Inputs must fit its spec and be the frame's size.
pub fn eval(
    spec: &'static OpSpec,
    params: &Params,
    inputs: &[&Image],
    frame: Frame,
) -> Result<Image, TexError> {
    let count = inputs.len();
    let fits = count >= usize::from(spec.inputs.min) && count <= usize::from(spec.inputs.max);
    if !fits || inputs.iter().any(|i| i.size() != frame.size) {
        return Err(TexError::BadInputs(spec.name));
    }
    (spec.kernel)(&Ctx {
        frame,
        spec,
        params,
        inputs,
    })
}

/// Shorthand for the tables: a float parameter.
pub(crate) const fn float(name: &'static str, min: f32, max: f32, default: f32) -> ParamSpec {
    ParamSpec {
        name,
        kind: ParamKind::Float { min, max, default },
    }
}

/// Shorthand for the tables: an integer parameter.
pub(crate) const fn int(name: &'static str, min: i32, max: i32, default: i32) -> ParamSpec {
    ParamSpec {
        name,
        kind: ParamKind::Int { min, max, default },
    }
}

/// Shorthand for the tables: a parameter that is one of some names.
pub(crate) const fn choice(
    name: &'static str,
    names: &'static [&'static str],
    default: u8,
) -> ParamSpec {
    ParamSpec {
        name,
        kind: ParamKind::Choice { names, default },
    }
}

/// Shorthand for the tables: a colour parameter.
pub(crate) const fn color(name: &'static str, default: [f32; 4]) -> ParamSpec {
    ParamSpec {
        name,
        kind: ParamKind::Color { default },
    }
}

/// Shorthand for the tables: a text parameter.
pub(crate) const fn text(name: &'static str, default: &'static str) -> ParamSpec {
    ParamSpec {
        name,
        kind: ParamKind::Text { default },
    }
}

/// Shorthand for the tables: exactly `n` inputs.
pub(crate) const fn exactly(n: u8) -> Inputs {
    Inputs { min: n, max: n }
}

#[cfg(test)]
mod tests;
