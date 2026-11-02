//! Operators that move images around the page instead of making them: named
//! store and load, and the outputs that say which image is which map.

use super::cost::Cost;
use super::{Channel, Ctx, OpSpec, Role, exactly, float, text};
use crate::error::TexError;
use crate::image::Image;

fn pass_on(ctx: &Ctx) -> Result<Image, TexError> {
    ctx.input(0).cloned()
}

/// The graph fills a load in from its store, so this only runs if something
/// calls the kernel directly.
fn unresolved(ctx: &Ctx) -> Result<Image, TexError> {
    Err(TexError::BadInputs(ctx.spec.name))
}

pub(super) const OPS: &[OpSpec] = &[
    OpSpec {
        name: "store",
        group: "Flow",
        inputs: exactly(1),
        params: &[text("name", "a")],
        role: Role::Store,
        kernel: pass_on,
        cost: Cost::Flat(0.0),
    },
    OpSpec {
        name: "load",
        group: "Flow",
        inputs: exactly(0),
        params: &[text("name", "a")],
        role: Role::Load,
        kernel: unresolved,
        cost: Cost::Flat(0.0),
    },
    OpSpec {
        name: "out-albedo",
        group: "Flow",
        inputs: exactly(1),
        params: &[],
        role: Role::Output(Channel::Albedo),
        kernel: pass_on,
        cost: Cost::Flat(0.0),
    },
    OpSpec {
        name: "out-height",
        group: "Flow",
        inputs: exactly(1),
        params: &[float("normal_strength", 0.0, 32.0, 4.0)],
        role: Role::Output(Channel::Height),
        kernel: pass_on,
        cost: Cost::Flat(0.0),
    },
    OpSpec {
        name: "out-specular",
        group: "Flow",
        inputs: exactly(1),
        params: &[],
        role: Role::Output(Channel::Specular),
        kernel: pass_on,
        cost: Cost::Flat(0.0),
    },
    OpSpec {
        name: "out-alpha",
        group: "Flow",
        inputs: exactly(1),
        params: &[],
        role: Role::Output(Channel::Alpha),
        kernel: pass_on,
        cost: Cost::Flat(0.0),
    },
];
