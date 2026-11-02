//! The recipe being edited in the Texture Studio: its page of blocks, the
//! selection and its own undo and redo. It stays out of [`super::Document`],
//! whose equality drives CSG and preview jobs, so a slider drag starts no level
//! work.
//!
//! An edit may leave the page not compiling, since a move on the way to somewhere
//! else often does. That is state, not failure: [`TexDoc::page`] says what is
//! wrong and with which blocks, and nothing here panics on any recipe, however
//! hostile the file it came from.

use std::sync::Arc;

use mscent_texgen::graph::{self, Graph, MAX_COLS, MAX_ROWS};
use mscent_texgen::image::Size;
use mscent_texgen::ops::{self, OpSpec, Params, Role, Value};
use mscent_texgen::recipe::{Block, MAX_BLOCKS, Recipe};

/// Undo steps kept at most.
pub(crate) const HISTORY: usize = 256;
/// What the undo steps may hold together, estimated. A step is a copy of the page,
/// and a page of strokes is not small: a stroke list is copied whole when it grows
/// while an earlier step still shares it, so 256 steps of a file at its size limit
/// would be hundreds of megabytes. Past this the oldest steps go first.
pub(crate) const HISTORY_BYTES: usize = 64 * 1024 * 1024;
/// What a block, with its parameters, is estimated to take besides its strokes.
const BLOCK_BYTES: usize = 256;

/// About how much memory a recipe takes, as far as undo is concerned: the strokes
/// are what can be large.
fn estimate(recipe: &Recipe) -> usize {
    let value = |v: &Value| match v {
        Value::Strokes(list) => list
            .iter()
            .map(|s| std::mem::size_of::<ops::Stroke>() + std::mem::size_of_val(&s.points[..]))
            .sum(),
        Value::Text(text) => text.len(),
        _ => 0,
    };
    let block = |b: &Block| BLOCK_BYTES + b.params.values().iter().map(value).sum::<usize>();
    recipe.blocks.iter().map(block).sum()
}

/// What is wrong with a page: the first problem, for reading, and every block
/// that has one, for marking.
#[derive(Clone, Debug, PartialEq)]
pub struct PageError {
    pub message: String,
    pub blocks: Vec<usize>,
}

/// The state undo and redo move between.
#[derive(Clone, Debug, PartialEq)]
struct Snapshot {
    recipe: Recipe,
    selected: Option<usize>,
    /// [`estimate`] of the recipe, taken once.
    bytes: usize,
}

/// The value a drag is changing, so that a whole drag is one undo step.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Live {
    Param(usize, usize),
    Gloss,
    /// Drawing a stroke on a block's strokes parameter (block, parameter).
    Stroke(usize, usize),
}

/// A recipe open in the studio.
#[derive(Debug)]
pub struct TexDoc {
    name: String,
    recipe: Recipe,
    /// The recipe as it is on disk, to know whether there is anything to save.
    saved: Recipe,
    selected: Option<usize>,
    past: Vec<Snapshot>,
    future: Vec<Snapshot>,
    live: Option<Live>,
    /// Changes with every edit, so whoever shows the recipe can tell it moved.
    revision: u64,
    /// Changes when the parameter widgets no longer show the page: a selection,
    /// an undo. A drag leaves it alone, or it would remake the slider being held.
    panel_epoch: u64,
    page: Result<Graph, PageError>,
}

fn compile(recipe: &Recipe) -> Result<Graph, PageError> {
    graph::compile(recipe).map_err(|e| PageError {
        message: e.to_string(),
        blocks: graph::problems(recipe)
            .into_iter()
            .map(|p| p.block)
            .collect(),
    })
}

/// What a new material starts from, found through the table: the first operator
/// that makes an image from nothing, fed to the albedo output.
fn starter_blocks() -> Vec<Block> {
    let generator = ops::all().find(|op| op.role == Role::Kernel && op.inputs.max == 0);
    let albedo = ops::all().find(|op| matches!(op.role, Role::Output(ops::Channel::Albedo)));
    let block = |spec: &'static OpSpec, y| Block {
        x: 0,
        y,
        w: 1,
        spec,
        params: Params::defaults(spec),
    };
    [generator.map(|g| block(g, 0)), albedo.map(|a| block(a, 1))]
        .into_iter()
        .flatten()
        .collect()
}

impl TexDoc {
    /// A recipe opened from `recipe`, which is untrusted: it may not compile.
    /// `gloss` is the manifest's, which has the last word over the recipe's.
    pub fn open(name: &str, mut recipe: Recipe, gloss: u8) -> Self {
        recipe.gloss = gloss;
        let page = compile(&recipe);
        Self {
            name: name.to_string(),
            saved: recipe.clone(),
            recipe,
            selected: None,
            past: Vec::new(),
            future: Vec::new(),
            live: None,
            revision: 0,
            panel_epoch: 0,
            page,
        }
    }

    /// A material that has no recipe yet (its pictures are PNG files), on the page
    /// a new one starts from. Nothing has been edited, so there is nothing to save.
    pub fn first_page(name: &str, size: Size, gloss: u8) -> Self {
        let recipe = Recipe {
            size,
            gloss,
            blocks: starter_blocks(),
        };
        let mut doc = Self::open(name, recipe, gloss);
        doc.selected = (!doc.recipe.blocks.is_empty()).then_some(0);
        doc
    }

    /// A new material: its first page, and nothing on disk to match.
    pub fn starter(name: &str, size: Size, gloss: u8) -> Self {
        let mut doc = Self::first_page(name, size, gloss);
        doc.saved.blocks.clear();
        doc
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn recipe(&self) -> &Recipe {
        &self.recipe
    }

    pub fn selected(&self) -> Option<usize> {
        self.selected
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn panel_epoch(&self) -> u64 {
        self.panel_epoch
    }

    /// The compiled page, or what is wrong with it.
    pub fn page(&self) -> &Result<Graph, PageError> {
        &self.page
    }

    pub fn dirty(&self) -> bool {
        self.recipe != self.saved
    }

    pub fn can_undo(&self) -> bool {
        !self.past.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.future.is_empty()
    }

    /// What is now on disk: `recipe` was written.
    pub fn mark_saved(&mut self, recipe: Recipe) {
        self.saved = recipe;
    }

    fn snapshot(&self) -> Snapshot {
        Snapshot {
            recipe: self.recipe.clone(),
            selected: self.selected,
            bytes: estimate(&self.recipe),
        }
    }

    /// Keeps the state before an edit for undo, and forgets anything to redo. The
    /// oldest steps go when there are too many or they hold too much; the newest
    /// always stays.
    fn checkpoint(&mut self) {
        self.past.push(self.snapshot());
        self.future.clear();
        let mut held: usize = self.past.iter().map(|s| s.bytes).sum();
        let mut oldest = 0;
        while self.past.len() - oldest > 1
            && (self.past.len() - oldest > HISTORY || held > HISTORY_BYTES)
        {
            held -= self.past[oldest].bytes;
            oldest += 1;
        }
        self.past.drain(..oldest);
    }

    fn changed(&mut self) {
        self.revision = self.revision.wrapping_add(1);
    }

    /// After a change to where blocks are or to which there are.
    fn page_changed(&mut self) {
        self.page = compile(&self.recipe);
        self.panel_epoch = self.panel_epoch.wrapping_add(1);
        self.changed();
    }

    fn block_mut(&mut self, block: usize) -> Result<&mut Block, String> {
        self.recipe
            .blocks
            .get_mut(block)
            .ok_or_else(|| format!("there is no block {block}"))
    }

    /// Selects a block; an index past the page clears the selection.
    pub fn select(&mut self, block: Option<usize>) {
        let block = block.filter(|&b| b < self.recipe.blocks.len());
        if self.selected != block {
            self.selected = block;
            self.live = None;
            self.panel_epoch = self.panel_epoch.wrapping_add(1);
            self.changed();
        }
    }

    /// The first free cell of column `x` at or below row `y`. No such cell, on a
    /// full column, leaves the block where it was asked for: the page says so.
    fn free_row(&self, x: u16, y: u16, w: u16) -> u16 {
        let taken = |row: u16| {
            self.recipe.blocks.iter().any(|b| {
                let (from, to) = (u32::from(b.x), u32::from(b.x) + u32::from(b.w));
                let (left, right) = (u32::from(x), u32::from(x) + u32::from(w));
                b.y == row && from < right && left < to
            })
        };
        (y..MAX_ROWS).find(|&row| !taken(row)).unwrap_or(y)
    }

    fn insert(&mut self, block: Block) -> Result<usize, String> {
        if self.recipe.blocks.len() >= MAX_BLOCKS {
            return Err(format!("a page holds at most {MAX_BLOCKS} blocks"));
        }
        self.checkpoint();
        self.live = None;
        self.recipe.blocks.push(block);
        let index = self.recipe.blocks.len() - 1;
        self.selected = Some(index);
        self.page_changed();
        Ok(index)
    }

    /// Adds an operator at a cell, or below it if that is taken, and selects it.
    /// It covers as many columns as it needs inputs.
    pub fn add(&mut self, spec: &'static OpSpec, x: u16, y: u16) -> Result<usize, String> {
        let w = u16::from(spec.inputs.min).max(1);
        let block = Block {
            x,
            y: self.free_row(x, y, w),
            w,
            spec,
            params: Params::defaults(spec),
        };
        self.insert(block)
    }

    /// Moves a block. Landing on another block is allowed, and shows as an error.
    pub fn move_block(&mut self, block: usize, x: u16, y: u16) -> Result<(), String> {
        let at = self.block_mut(block)?;
        if (at.x, at.y) == (x, y) {
            return Ok(());
        }
        self.checkpoint();
        self.live = None;
        let at = self.block_mut(block)?;
        (at.x, at.y) = (x, y);
        self.page_changed();
        Ok(())
    }

    /// Sets how many columns a block covers, at least one.
    pub fn resize(&mut self, block: usize, w: u16) -> Result<(), String> {
        let w = w.max(1);
        let at = self.block_mut(block)?;
        if at.w == w {
            return Ok(());
        }
        self.checkpoint();
        self.live = None;
        self.block_mut(block)?.w = w;
        self.page_changed();
        Ok(())
    }

    pub fn delete(&mut self, block: usize) -> Result<(), String> {
        self.block_mut(block)?;
        self.checkpoint();
        self.live = None;
        self.recipe.blocks.remove(block);
        self.selected = match self.selected {
            Some(s) if s == block => None,
            Some(s) if s > block => Some(s - 1),
            other => other,
        };
        self.page_changed();
        Ok(())
    }

    /// A copy of a block in the next free cell below it, selected.
    pub fn duplicate(&mut self, block: usize) -> Result<usize, String> {
        let mut copy = self.block_mut(block)?.clone();
        copy.y = self.free_row(copy.x, copy.y, copy.w);
        self.insert(copy)
    }

    /// Sets parameter `index` of a block. A `live` edit is part of a drag: the
    /// drag is one undo step, ended by the next edit that is not live, and the
    /// value it leaves is the one that edit sets. A value the table does not
    /// accept is an error and changes nothing.
    pub fn set_param(
        &mut self,
        block: usize,
        index: usize,
        value: Value,
        live: bool,
    ) -> Result<(), String> {
        let at = self.block_mut(block)?;
        let spec = at.spec;
        let param = spec
            .params
            .get(index)
            .ok_or_else(|| format!("{} has no parameter {index}", spec.name))?;
        if !param.kind.accepts(&value) {
            return Err(format!("{}: value out of range", param.name));
        }
        let unchanged = at.params.values().get(index) == Some(&value);
        let key = Live::Param(block, index);
        if unchanged {
            self.live = self.live.filter(|_| live);
            return Ok(());
        }
        if self.live != Some(key) {
            self.checkpoint();
        }
        self.live = live.then_some(key);
        let affects_page = matches!(value, Value::Text(_));
        self.block_mut(block)?
            .params
            .set(spec, param.name, value)
            .map_err(|e| e.to_string())?;
        // Only a name can change how blocks connect; the rest need no compile,
        // which matters to a drag.
        if affects_page {
            self.page = compile(&self.recipe);
        }
        self.changed();
        Ok(())
    }

    /// Empties a strokes parameter.
    pub fn clear_strokes(&mut self, block: usize, index: usize) -> Result<(), String> {
        self.set_param(block, index, Value::Strokes(Arc::default()), false)?;
        // The panel says how many there are.
        self.panel_epoch = self.panel_epoch.wrapping_add(1);
        Ok(())
    }

    /// Sets the material's gloss. `live` as in [`Self::set_param`].
    pub fn set_gloss(&mut self, gloss: u8, live: bool) {
        if self.recipe.gloss == gloss {
            self.live = self.live.filter(|_| live);
            return;
        }
        if self.live != Some(Live::Gloss) {
            self.checkpoint();
        }
        self.live = live.then_some(Live::Gloss);
        self.recipe.gloss = gloss;
        self.changed();
    }

    fn restore(&mut self, state: Snapshot) {
        self.recipe = state.recipe;
        self.selected = state.selected;
        self.live = None;
        self.page_changed();
    }

    pub fn undo(&mut self) -> bool {
        let Some(state) = self.past.pop() else {
            return false;
        };
        self.future.push(self.snapshot());
        self.restore(state);
        true
    }

    pub fn redo(&mut self) -> bool {
        let Some(state) = self.future.pop() else {
            return false;
        };
        self.past.push(self.snapshot());
        self.restore(state);
        true
    }
}

/// Where a block may be put so that it stays on the page, whatever was asked.
pub fn clamp_to_page(x: i32, y: i32, w: u16) -> (u16, u16) {
    let right = i32::from(MAX_COLS) - i32::from(w.min(MAX_COLS));
    let x = x.clamp(0, right.max(0));
    let y = y.clamp(0, i32::from(MAX_ROWS) - 1);
    (x as u16, y as u16)
}

#[path = "texdoc_strokes.rs"]
mod strokes;
pub use strokes::{Growth, STROKE_POINTS};

#[cfg(test)]
#[path = "texdoc_tests.rs"]
mod texdoc_tests;
