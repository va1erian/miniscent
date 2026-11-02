//! Everything wrong with a page, block by block. [`super::compile`] stops at the
//! first problem, which is all a bake needs; an editor marks every broken block.
//! A test holds the two to the same answer: no problems if and only if it compiles.

use std::collections::HashMap;

use super::{MAX_COLS, MAX_ROWS, graph_error, inputs_of, ordered, outputs_of, slot_name};
use crate::ops::Role;
use crate::recipe::{Block, MAX_BLOCKS, Recipe};

/// A block and what is wrong with it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Problem {
    pub block: usize,
    pub why: String,
}

/// Each block's first problem, by block index.
struct Found(Vec<Option<String>>);

impl Found {
    fn note(&mut self, block: usize, why: impl ToString) {
        if let Some(slot @ None) = self.0.get_mut(block) {
            *slot = Some(why.to_string());
        }
    }
}

fn on_page(b: &Block) -> bool {
    let right = b.x.checked_add(b.w);
    b.w > 0 && right.is_some_and(|r| r <= MAX_COLS) && b.y < MAX_ROWS
}

fn note_overlaps(blocks: &[Block], found: &mut Found) {
    for (i, b) in blocks.iter().enumerate() {
        for (j, other) in blocks.iter().enumerate().take(i) {
            if other.y == b.y && other.x < b.x + b.w && b.x < other.x + other.w {
                found.note(i, graph_error(b, "overlaps another block"));
                found.note(j, graph_error(other, "overlaps another block"));
            }
        }
    }
}

fn note_stores<'a>(blocks: &'a [Block], found: &mut Found) -> HashMap<&'a str, usize> {
    let mut stores = HashMap::new();
    for (i, b) in blocks.iter().enumerate() {
        if b.spec.role != Role::Store {
            continue;
        }
        match slot_name(b) {
            None => found.note(i, graph_error(b, "has no name")),
            Some(name) if stores.contains_key(name) => {
                found.note(i, graph_error(b, "stores a name already used"));
            }
            Some(name) => {
                stores.insert(name, i);
            }
        }
    }
    stores
}

fn note_connections(blocks: &[Block], stores: &HashMap<&str, usize>, found: &mut Found) {
    let mut inputs = Vec::with_capacity(blocks.len());
    for (i, b) in blocks.iter().enumerate() {
        match inputs_of(blocks, b, stores) {
            Ok(list) => inputs.push(list),
            Err(e) => {
                found.note(i, e);
                inputs.push(Vec::new());
            }
        }
    }
    let order = ordered(blocks, &inputs);
    for i in (0..blocks.len()).filter(|i| !order.contains(i)) {
        found.note(i, "store and load blocks form a cycle");
    }
}

/// Every block with a problem, by block index, with its first problem. The later
/// checks assume blocks are on the page, so a block off it is reported alone.
pub fn problems(recipe: &Recipe) -> Vec<Problem> {
    let blocks = &recipe.blocks;
    let mut found = Found(vec![None; blocks.len()]);
    for (i, b) in blocks.iter().enumerate() {
        if i >= MAX_BLOCKS {
            found.note(i, "too many blocks");
        } else if !on_page(b) {
            found.note(i, graph_error(b, "off the page"));
        }
    }
    if blocks.iter().all(on_page) {
        note_overlaps(blocks, &mut found);
        let stores = note_stores(blocks, &mut found);
        note_connections(blocks, &stores, &mut found);
        if let Err(e) = outputs_of(blocks) {
            for (i, b) in blocks.iter().enumerate() {
                if matches!(b.spec.role, Role::Output(_)) {
                    found.note(i, &e);
                }
            }
        }
    }
    found
        .0
        .into_iter()
        .enumerate()
        .filter_map(|(block, why)| Some(Problem { block, why: why? }))
        .collect()
}
