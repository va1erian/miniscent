//! What a run will ask for, worked out from the page before any kernel runs: its
//! time and its peak of memory. Neither looks at a clock, a cache or a pixel, so a
//! page is accepted or refused the same way by the editor, a bake and every machine.

use super::Graph;
use crate::error::TexError;
use crate::image::Size;
use crate::ops::{self, Role};
use crate::recipe::Recipe;

/// Most bytes of images one evaluation may hold at once. Inputs are freed as
/// soon as their last reader has run, so only a hostile page gets near this.
pub const MAX_LIVE_BYTES: usize = 512 * 1024 * 1024;

/// Most nanoseconds of estimated work in one recipe, at the size it is run at: the
/// worst page that passes takes about 5 s on the machine the costs were measured
/// on. The game makes recipes on one thread at startup, with no way to cancel, and
/// 30 `dots`, the dearest ordinary block, still fit at 1024 px.
pub const MAX_RECIPE_NS: u64 = 5_000_000_000;

/// The estimated nanoseconds of the kernels of a page at `size`, whatever a cache
/// may hold, so the answer does not depend on one.
pub fn page_cost(recipe: &Recipe, graph: &Graph, size: Size) -> u64 {
    recipe
        .blocks
        .iter()
        .enumerate()
        .filter(|(_, b)| b.spec.role == Role::Kernel)
        .map(|(i, b)| ops::cost_ns(b.spec, &b.params, graph.inputs_of(i).len(), size))
        .fold(0, u64::saturating_add)
}

/// Blocks that hold an image, by the block that made the image. A block that only
/// passes its input on holds the same image, which goes when its last holder does.
struct Holders {
    owners: Vec<usize>,
    holding: Vec<usize>,
    live: usize,
}

impl Holders {
    fn hold(&mut self, block: usize, owner: usize) {
        if let (Some(slot), Some(count)) = (self.owners.get_mut(block), self.holding.get_mut(owner))
        {
            *slot = owner;
            *count += 1;
            self.live += usize::from(*count == 1);
        }
    }

    fn release(&mut self, block: usize) {
        let owner = self.owners.get(block).copied().unwrap_or(usize::MAX);
        if let Some(count) = self.holding.get_mut(owner) {
            *count -= 1;
            self.live -= usize::from(*count == 0);
        }
    }
}

/// The most images a run holds at once, by simulating its order and readers with
/// each kernel block counted as its own image. A cache could serve two identical
/// blocks with one image, but a run with no cache cannot, and the answer must be
/// the same for both.
pub fn peak_images(recipe: &Recipe, graph: &Graph, keep: &[usize]) -> usize {
    let count = recipe.blocks.len();
    let mut readers = vec![0usize; count];
    for &input in graph.inputs.iter().flatten() {
        if let Some(r) = readers.get_mut(input) {
            *r += 1;
        }
    }
    let kept = |i: usize| keep.contains(&i) || graph.outputs.iter().any(|o| o.1 == i);
    let mut holders = Holders {
        owners: vec![usize::MAX; count],
        holding: vec![0; count],
        live: 0,
    };
    let mut peak = 0;
    for &i in &graph.order {
        let list = graph.inputs_of(i);
        let makes_one = recipe
            .blocks
            .get(i)
            .is_some_and(|b| b.spec.role == Role::Kernel);
        let passed_on = list.first().and_then(|&j| holders.owners.get(j)).copied();
        holders.hold(i, if makes_one { i } else { passed_on.unwrap_or(i) });
        peak = peak.max(holders.live);
        for &j in list {
            if let Some(r) = readers.get_mut(j) {
                *r = r.saturating_sub(1);
                if *r == 0 && !kept(j) {
                    holders.release(j);
                }
            }
        }
        if readers.get(i) == Some(&0) && !kept(i) {
            holders.release(i);
        }
    }
    peak
}

/// Refuses a run that would hold too much or take too long, before it makes a pixel.
pub fn check(recipe: &Recipe, graph: &Graph, size: Size, keep: &[usize]) -> Result<(), TexError> {
    let bytes = peak_images(recipe, graph, keep).saturating_mul(size.bytes());
    if bytes > MAX_LIVE_BYTES || page_cost(recipe, graph, size) > MAX_RECIPE_NS {
        return Err(TexError::TooLarge);
    }
    Ok(())
}
