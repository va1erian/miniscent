//! From a page of blocks to images. A block reads the blocks on the row directly
//! above it that overlap its columns, left to right, which is how .kkrieger's tool
//! wired operators: there are no wires to store. Named `store` and `load` blocks
//! reach further, so cycles are possible only through them, and are rejected.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::error::TexError;
use crate::image::{Frame, Image, Size};
use crate::ops::{self, Channel, Role, Value};
use crate::recipe::{Block, MAX_BLOCKS, Recipe};

/// Most columns and rows on a page.
pub const MAX_COLS: u16 = 64;
pub const MAX_ROWS: u16 = 256;

/// Most bytes of images one evaluation may hold at once. Inputs are freed as
/// soon as their last reader has run, so only a hostile page gets near this.
pub const MAX_LIVE_BYTES: usize = 512 * 1024 * 1024;

/// A page, checked: every block's inputs, and an order to run them in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Graph {
    /// By block index: the blocks whose images it reads.
    inputs: Vec<Vec<usize>>,
    /// Block indices, each after its inputs.
    order: Vec<usize>,
    outputs: Vec<(Channel, usize)>,
}

fn graph_error(block: &Block, why: &str) -> TexError {
    TexError::Graph(format!(
        "{} at {},{}: {why}",
        block.spec.name, block.x, block.y
    ))
}

fn check_layout(blocks: &[Block]) -> Result<(), TexError> {
    if blocks.len() > MAX_BLOCKS {
        return Err(TexError::Graph("too many blocks".to_string()));
    }
    for (i, b) in blocks.iter().enumerate() {
        let right = b.x.checked_add(b.w);
        let fits = right.is_some_and(|r| r <= MAX_COLS);
        if b.w == 0 || !fits || b.y >= MAX_ROWS {
            return Err(graph_error(b, "off the page"));
        }
        for other in &blocks[..i] {
            if other.y == b.y && other.x < b.x + b.w && b.x < other.x + other.w {
                return Err(graph_error(b, "overlaps another block"));
            }
        }
    }
    Ok(())
}

/// The name a `store` or `load` carries.
fn slot_name(block: &Block) -> Option<&str> {
    block.params.values().iter().find_map(|v| match v {
        Value::Text(t) => Some(t.as_str()),
        _ => None,
    })
}

fn stores_of(blocks: &[Block]) -> Result<HashMap<&str, usize>, TexError> {
    let mut stores = HashMap::new();
    for (i, b) in blocks.iter().enumerate() {
        if b.spec.role == Role::Store {
            let name = slot_name(b).ok_or_else(|| graph_error(b, "has no name"))?;
            if stores.insert(name, i).is_some() {
                return Err(graph_error(b, "stores a name already used"));
            }
        }
    }
    Ok(stores)
}

/// The blocks on the row above `block` under its columns, left to right.
fn above(blocks: &[Block], block: &Block) -> Vec<usize> {
    let Some(row) = block.y.checked_sub(1) else {
        return Vec::new();
    };
    let mut found: Vec<usize> = (0..blocks.len())
        .filter(|&i| {
            let b = &blocks[i];
            b.y == row && b.x < block.x + block.w && block.x < b.x + b.w
        })
        .collect();
    found.sort_by_key(|&i| blocks[i].x);
    found
}

fn inputs_of(
    blocks: &[Block],
    block: &Block,
    stores: &HashMap<&str, usize>,
) -> Result<Vec<usize>, TexError> {
    if block.spec.role == Role::Load {
        let name = slot_name(block).ok_or_else(|| graph_error(block, "has no name"))?;
        let store = stores
            .get(name)
            .ok_or_else(|| graph_error(block, "loads a name nothing stores"))?;
        return Ok(vec![*store]);
    }
    let limits = block.spec.inputs;
    // A generator below another block starts a new chain.
    if limits.max == 0 {
        return Ok(Vec::new());
    }
    let found = above(blocks, block);
    if found.len() < usize::from(limits.min) {
        return Err(graph_error(block, "needs more inputs above it"));
    }
    if found.len() > usize::from(limits.max) {
        return Err(graph_error(block, "has too many inputs above it"));
    }
    Ok(found)
}

/// Each block after its inputs, or an error for a cycle. Ready blocks run in page
/// order, so the order is the same on every run.
fn order_of(blocks: &[Block], inputs: &[Vec<usize>]) -> Result<Vec<usize>, TexError> {
    let mut waiting: Vec<usize> = inputs.iter().map(Vec::len).collect();
    let mut readers = vec![Vec::new(); blocks.len()];
    for (i, list) in inputs.iter().enumerate() {
        for &input in list {
            readers[input].push(i);
        }
    }
    let mut ready: Vec<usize> = (0..blocks.len())
        .rev()
        .filter(|&i| waiting[i] == 0)
        .collect();
    let mut order = Vec::with_capacity(blocks.len());
    while let Some(i) = ready.pop() {
        order.push(i);
        for &reader in readers[i].iter().rev() {
            waiting[reader] -= 1;
            if waiting[reader] == 0 {
                ready.push(reader);
            }
        }
    }
    if order.len() != blocks.len() {
        return Err(TexError::Graph(
            "store and load blocks form a cycle".to_string(),
        ));
    }
    Ok(order)
}

fn outputs_of(blocks: &[Block]) -> Result<Vec<(Channel, usize)>, TexError> {
    let mut outputs: Vec<(Channel, usize)> = Vec::new();
    for (i, b) in blocks.iter().enumerate() {
        if let Role::Output(channel) = b.spec.role {
            if outputs.iter().any(|o| o.0 == channel) {
                return Err(graph_error(b, "repeats an output"));
            }
            outputs.push((channel, i));
        }
    }
    Ok(outputs)
}

/// Checks a page and works out how its blocks connect.
pub fn compile(recipe: &Recipe) -> Result<Graph, TexError> {
    let blocks = &recipe.blocks;
    check_layout(blocks)?;
    let stores = stores_of(blocks)?;
    let inputs = blocks
        .iter()
        .map(|b| inputs_of(blocks, b, &stores))
        .collect::<Result<Vec<_>, _>>()?;
    let order = order_of(blocks, &inputs)?;
    Ok(Graph {
        order,
        outputs: outputs_of(blocks)?,
        inputs,
    })
}

impl Graph {
    /// The block holding a map, if the page has that output.
    pub fn output(&self, channel: Channel) -> Option<usize> {
        self.outputs.iter().find(|o| o.0 == channel).map(|o| o.1)
    }

    /// The blocks a block reads.
    pub fn inputs_of(&self, block: usize) -> &[usize] {
        self.inputs.get(block).map_or(&[], Vec::as_slice)
    }
}

/// Results of finished blocks, so that changing one parameter re-runs only what
/// depends on it. Keys hash what makes a result what it is. Oldest out.
#[derive(Debug)]
pub struct Cache {
    budget: usize,
    bytes: usize,
    clock: u64,
    entries: HashMap<u64, (Arc<Image>, u64)>,
}

impl Cache {
    pub fn new(budget_bytes: usize) -> Self {
        Self {
            budget: budget_bytes,
            bytes: 0,
            clock: 0,
            entries: HashMap::new(),
        }
    }

    fn get(&mut self, key: u64) -> Option<Arc<Image>> {
        self.clock += 1;
        let clock = self.clock;
        self.entries.get_mut(&key).map(|e| {
            e.1 = clock;
            Arc::clone(&e.0)
        })
    }

    fn put(&mut self, key: u64, image: Arc<Image>) {
        self.clock += 1;
        self.bytes += image.size().bytes();
        if let Some(old) = self.entries.insert(key, (image, self.clock)) {
            self.bytes -= old.0.size().bytes();
        }
        while self.bytes > self.budget {
            let oldest = self.entries.iter().min_by_key(|e| e.1.1).map(|e| *e.0);
            let Some(removed) = oldest.and_then(|k| self.entries.remove(&k)) else {
                break;
            };
            self.bytes -= removed.0.size().bytes();
        }
    }

    /// How many results it holds.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// FNV-1a over what identifies a result.
struct Hash(u64);

impl Hash {
    fn eat(&mut self, bytes: &[u8]) {
        for b in bytes {
            self.0 = (self.0 ^ u64::from(*b)).wrapping_mul(0x0000_0100_0000_01b3);
        }
    }

    fn eat_floats(&mut self, floats: &[f32]) {
        for f in floats {
            self.eat(&f.to_bits().to_le_bytes());
        }
    }

    fn value(&mut self, value: &Value) {
        match value {
            Value::Float(v) => self.eat_floats(&[*v]),
            Value::Int(v) => self.eat(&v.to_le_bytes()),
            Value::Choice(v) => self.eat(&[*v]),
            Value::Color(c) => self.eat_floats(c),
            Value::Text(t) => {
                self.eat(&t.len().to_le_bytes());
                self.eat(t.as_bytes());
            }
            Value::Strokes(strokes) => {
                for s in strokes.iter() {
                    self.eat_floats(&[s.radius, s.hardness]);
                    self.eat_floats(&s.color);
                    self.eat(&s.points.len().to_le_bytes());
                    for p in &s.points {
                        self.eat_floats(p);
                    }
                }
            }
        }
    }
}

fn key_of(block: &Block, inputs: &[u64], size: Size) -> u64 {
    let mut h = Hash(0xcbf2_9ce4_8422_2325);
    h.eat(block.spec.name.as_bytes());
    h.eat(&size.width().to_le_bytes());
    h.eat(&size.height().to_le_bytes());
    for v in block.params.values() {
        h.value(v);
    }
    for k in inputs {
        h.eat(&k.to_le_bytes());
    }
    h.0
}

/// Options for [`run`].
#[derive(Default)]
pub struct RunOptions<'a> {
    /// Reuses and keeps results.
    pub cache: Option<&'a mut Cache>,
    /// When set, the run stops with [`TexError::Cancelled`].
    pub cancel: Option<&'a AtomicBool>,
    /// Blocks whose images the caller wants, on top of the outputs.
    pub keep: &'a [usize],
}

/// A finished run: the images of the outputs and of the blocks asked for.
#[derive(Debug)]
pub struct Outcome {
    images: Vec<Option<Arc<Image>>>,
}

impl Outcome {
    /// The image of a block, if it was kept.
    pub fn image(&self, block: usize) -> Option<&Arc<Image>> {
        self.images.get(block)?.as_ref()
    }
}

/// The distinct images held, since passing a block on shares its input's.
fn live_images(images: &[Option<Arc<Image>>]) -> usize {
    let mut held: Vec<*const Image> = images.iter().flatten().map(Arc::as_ptr).collect();
    held.sort_unstable();
    held.dedup();
    held.len()
}

/// Runs the page at `size`, which may differ from the recipe's own: a draft is
/// the same recipe, smaller.
pub fn run(
    recipe: &Recipe,
    graph: &Graph,
    size: Size,
    options: RunOptions,
) -> Result<Outcome, TexError> {
    let RunOptions {
        mut cache,
        cancel,
        keep,
    } = options;
    let count = recipe.blocks.len();
    let mut readers = vec![0usize; count];
    for list in &graph.inputs {
        for &i in list {
            readers[i] += 1;
        }
    }
    let kept = |i: usize| keep.contains(&i) || graph.outputs.iter().any(|o| o.1 == i);
    let mut images: Vec<Option<Arc<Image>>> = vec![None; count];
    let mut keys = vec![0u64; count];
    for &i in &graph.order {
        if cancel.is_some_and(|c| c.load(Ordering::Relaxed)) {
            return Err(TexError::Cancelled);
        }
        let block = recipe.blocks.get(i).ok_or(TexError::BadSize)?;
        let list = graph.inputs.get(i).map_or(&[][..], Vec::as_slice);
        let held: Vec<Arc<Image>> = list.iter().filter_map(|&j| images[j].clone()).collect();
        if held.len() != list.len() {
            return Err(graph_error(block, "input missing"));
        }
        let input_keys: Vec<u64> = list.iter().map(|&j| keys[j]).collect();
        keys[i] = key_of(block, &input_keys, size);
        images[i] = Some(match block.spec.role {
            Role::Kernel => {
                let hit = cache.as_deref_mut().and_then(|c| c.get(keys[i]));
                match hit {
                    Some(image) => image,
                    None => {
                        let live = live_images(&images).saturating_add(1);
                        if live.saturating_mul(size.bytes()) > MAX_LIVE_BYTES {
                            return Err(TexError::TooLarge);
                        }
                        let refs: Vec<&Image> = held.iter().map(Arc::as_ref).collect();
                        let image = Arc::new(ops::eval(
                            block.spec,
                            &block.params,
                            &refs,
                            Frame::new(size),
                        )?);
                        if let Some(c) = cache.as_deref_mut() {
                            c.put(keys[i], Arc::clone(&image));
                        }
                        image
                    }
                }
            }
            _ => held
                .into_iter()
                .next()
                .ok_or_else(|| graph_error(block, "nothing to pass on"))?,
        });
        for &j in list {
            readers[j] -= 1;
            if readers[j] == 0 && !kept(j) {
                images[j] = None;
            }
        }
        if readers[i] == 0 && !kept(i) {
            images[i] = None;
        }
    }
    Ok(Outcome { images })
}

#[cfg(test)]
#[path = "graph_tests.rs"]
mod graph_tests;
