//! What the workspace's widgets say.

use mscent_texgen::ops::Value;

use crate::texstudio::{Stroking, Tab};

/// A change to the paint tool's brush.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BrushEdit {
    Radius(f32),
    Hardness(f32),
    Color([f32; 3]),
}

/// A message from the workspace.
#[derive(Clone)]
pub enum TexMsg {
    /// A row of the material list.
    Open(String),
    /// The New button: shows the form that asks for a name and a size.
    AskNew,
    /// The form's Create button, or Enter in it.
    NewSubmit,
    NewCancel,
    /// The answers to the question before a stock material is replaced.
    ConfirmOverwrite,
    CancelOverwrite,
    Create {
        name: String,
        side: u32,
    },
    Select(Option<usize>),
    /// A block was dragged.
    Move {
        block: usize,
        x: u16,
        y: u16,
    },
    /// A block's right edge was dragged.
    Resize {
        block: usize,
        w: u16,
    },
    /// The right button on the page, at the node's pixel; what it is over.
    PageMenu {
        x: i32,
        y: i32,
        cell: (u16, u16),
        block: Option<usize>,
    },
    /// Adds the operator of this table index at a cell.
    Add {
        op: usize,
        cell: (u16, u16),
    },
    Delete,
    Duplicate,
    Param {
        block: usize,
        index: usize,
        value: Value,
        live: bool,
    },
    /// One of a colour's channels was typed.
    Channel {
        block: usize,
        index: usize,
        channel: usize,
        value: f32,
    },
    ClearStrokes {
        block: usize,
        index: usize,
    },
    /// The pointer on the preview, drawing.
    Paint(Stroking),
    Brush(BrushEdit),
    Gloss {
        value: u8,
        live: bool,
    },
    Tab(Tab),
    Tiled(bool),
    Apply,
    Save,
}
