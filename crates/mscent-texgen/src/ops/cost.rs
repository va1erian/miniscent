//! What an operator costs to run, worked out from the recipe and never a clock,
//! so that whether a page is accepted is the same on every machine.
//!
//! Figures are nanoseconds per texel on the worst parameters, measured on the
//! release profile by `cargo run --release -p mscent-texgen --example texgen_costs`,
//! rounded up. They are estimates of time on one machine and promises of nothing
//! on another; what matters is that they rank pages the same everywhere and bound
//! what a hostile page can ask for.

use super::{OpSpec, Params, Stroke, Value};
use crate::image::Size;

/// How an operator's cost per texel is found.
#[derive(Clone, Copy, Debug)]
pub enum Cost {
    /// The same on any parameters that matter: the worst of them, measured.
    Flat(f32),
    /// Depends on parameters such as octaves or strokes.
    Worked(fn(&Knobs) -> f64),
}

/// What a [`Cost::Worked`] may look at: everything but pixels.
pub struct Knobs<'a> {
    pub spec: &'static OpSpec,
    pub params: &'a Params,
    /// How many images the block reads, from the graph.
    pub inputs: usize,
    pub size: Size,
}

impl Knobs<'_> {
    pub fn int(&self, name: &str) -> i32 {
        match self.params.named(self.spec, name) {
            Some(Value::Int(v)) => *v,
            _ => 0,
        }
    }

    pub fn float(&self, name: &str) -> f32 {
        match self.params.named(self.spec, name) {
            Some(Value::Float(v)) => *v,
            _ => 0.0,
        }
    }

    pub fn strokes(&self, name: &str) -> &[Stroke] {
        match self.params.named(self.spec, name) {
            Some(Value::Strokes(v)) => v,
            _ => &[],
        }
    }
}

/// Nanoseconds one block of `spec` is expected to take at `size`, reading `inputs`
/// images. Saturates: a page of forged numbers is too large, not an overflow.
pub fn cost_ns(spec: &'static OpSpec, params: &Params, inputs: usize, size: Size) -> u64 {
    let per_texel = match spec.cost {
        Cost::Flat(ns) => f64::from(ns),
        Cost::Worked(of) => of(&Knobs {
            spec,
            params,
            inputs,
            size,
        }),
    };
    (per_texel * size.texels() as f64) as u64
}
