//! The `mountains` line: a ridged-noise range, built when the text is read.
//!
//! ```text
//! mountains <order> <material> <min x y z> <max x y z> <cell> <peak> <seed> [clear <ax ay> <bx by> <radius>] [edge <units>]
//! mountains 3 4  -128 100 -2  128 356 60  1 40 7     # voxel material 4 is rock
//! ```
//!
//! `clear` leaves a road flat at the range's ground, from `a` to `b` in plan, so
//! one range can be a room's whole ground with a way through it. `edge` is how far
//! in from the box's sides the range reaches full height (by default 0.3 of each).
//!
//! The line is a recipe, not data: reading it runs [`mountains`], and the volume
//! is stored in the binary level like any other. `cell` is 0.125, 0.25, 0.5, 1, 2 or
//! 4 (a far range wants the coarse ones: its cells are a pixel or less). Dumping a
//! level cannot turn a volume back into this line, since a sculpted range no longer
//! is one, so `--dump-level` refuses a level with volumes rather than drop them.

use mscent_math::Vec3d;

use super::words::{Res, Words};
use crate::entity::Level;
use crate::voxel::{
    CellSize, Clearing, MAX_TOTAL_CHUNKS, MAX_VOLUMES, MountainRange, Volume, mountains,
};

fn cell_of(units: f64) -> Res<CellSize> {
    [
        CellSize::Eighth,
        CellSize::Quarter,
        CellSize::Half,
        CellSize::One,
        CellSize::Two,
        CellSize::Four,
    ]
    .into_iter()
    .find(|c| c.units() == units)
    .ok_or_else(|| "the cell must be 0.125, 0.25, 0.5, 1, 2 or 4".to_string())
}

fn chunks_in(volumes: &[Volume]) -> usize {
    let each = |v: &Volume| {
        v.chunks()
            .iter()
            .map(|&c| usize::from(c))
            .product::<usize>()
    };
    volumes.iter().map(each).sum()
}

/// Reads the rest of a `mountains` line and builds the volume. Refuses a range
/// that overlaps a volume the level has, or an order a brush uses.
pub(super) fn read(w: &mut Words, level: &Level) -> Res<Volume> {
    let order: u32 = w.parse("the order")?;
    let material: u8 = w.parse("the voxel material")?;
    let (min, max): (Vec3d, Vec3d) = (w.v3d("the min corner")?, w.v3d("the max corner")?);
    let cell = cell_of(w.f64("the cell")?)?;
    let peak = w.f64("the peak height")?;
    let seed: u32 = w.parse("the seed")?;
    let clear = clearing(w)?;
    let edge = keyword(w, "edge")
        .then(|| w.f64("the edge width"))
        .transpose()?;
    if level.volumes.len() >= MAX_VOLUMES || chunks_in(&level.volumes) >= MAX_TOTAL_CHUNKS {
        return Err("too many voxel volumes".to_string());
    }
    if level.brushes.iter().any(|b| b.order() == order) {
        return Err(format!("order {order} is used by a brush"));
    }
    let range = MountainRange {
        min,
        max,
        cell,
        peak,
        seed,
        material,
        order,
        clear,
        edge,
    };
    let volume = mountains(&range).map_err(|e| format!("bad mountains: {e}"))?;
    if level
        .volumes
        .iter()
        .any(|v| v.overlaps(&volume) || v.order() == order)
    {
        return Err("the range overlaps another volume or shares its order".to_string());
    }
    if chunks_in(&level.volumes) + chunks_in(std::slice::from_ref(&volume)) > MAX_TOTAL_CHUNKS {
        return Err("too many voxel chunks in the level".to_string());
    }
    Ok(volume)
}

/// The optional `clear <ax ay> <bx by> <radius>` at the end of a `mountains` line.
fn clearing(w: &mut Words) -> Res<Option<Clearing>> {
    if !keyword(w, "clear") {
        return Ok(None);
    }
    let mut xy = |what: &str| -> Res<[f64; 2]> { Ok([w.f64(what)?, w.f64(what)?]) };
    let (a, b) = (xy("the road's start")?, xy("the road's end")?);
    let radius = w.f64("the road's half width")?;
    Ok(Some(Clearing { a, b, radius }))
}

/// Whether the next word is `name`, taking it if so.
fn keyword(w: &mut Words, name: &str) -> bool {
    let next = w.rest.clone().next() == Some(name);
    if next {
        w.rest.next();
    }
    next
}
