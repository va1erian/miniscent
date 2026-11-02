//! Volumes in the level file: section 5 of `format.rs`.
//!
//! ```text
//! count u32
//! volume*: order u32, op u8, cell u8, origin i32*3, chunks u16*3, chunk*
//! chunk:   kind 0 (uniform): delta i8, material u8
//!          kind 1 (detail):  plane of deltas, plane of materials
//! plane:   len u32, then len bytes: exactly 4096 is the samples as they are,
//!          any other multiple of 3 is runs of (count u16, value u8)
//! ```
//!
//! Only the changes are stored; the loader resolves them against the brushes.
//! A small file can ask for far more memory than it holds, since one run covers
//! a whole plane, so detail chunks are charged against a budget before each is
//! allocated, and every plane must decode to exactly one chunk's samples.

use std::sync::Arc;

use super::chunk::{Chunk, ChunkData};
use super::volume::{CellSize, Volume};
use super::{CHUNK_SAMPLES, MAX_CHUNKS_PER_AXIS, MAX_DELTA};
use crate::brush::{Brush, Op};
use crate::format::FormatError;
use crate::format_wire::{Cursor, len_u32, put_u16, put_u32};

/// Most volumes in a level.
pub const MAX_VOLUMES: usize = 256;
/// Most detail chunks in a level: 128 MiB of samples.
pub const MAX_DETAIL_CHUNKS: usize = 16_384;

/// Smallest volume record: order, op, cell, origin and chunks.
const VOLUME_HEAD: usize = 4 + 1 + 1 + 12 + 6;
/// Smallest chunk record: a uniform one.
const CHUNK_MIN: usize = 3;
const KIND_UNIFORM: u8 = 0;
const KIND_DETAIL: u8 = 1;
/// Bytes of one run.
const RUN_BYTES: usize = 3;

pub(crate) fn write_volumes(volumes: &[Volume]) -> Vec<u8> {
    let mut out = Vec::new();
    put_u32(&mut out, len_u32(volumes.len()));
    for v in volumes {
        write_volume(&mut out, v);
    }
    out
}

fn write_volume(out: &mut Vec<u8>, v: &Volume) {
    let lattice = &v.lattice;
    put_u32(out, lattice.order);
    out.push(match lattice.op {
        Op::Add => 0,
        Op::Subtract => 1,
    });
    out.push(lattice.cell.code());
    for o in lattice.origin {
        out.extend_from_slice(&o.to_le_bytes());
    }
    for c in lattice.chunks {
        put_u16(out, c);
    }
    for chunk in v.table.iter() {
        write_chunk(out, chunk);
    }
}

fn write_chunk(out: &mut Vec<u8>, chunk: &Chunk) {
    match chunk {
        Chunk::Uniform { delta, material } => {
            out.extend_from_slice(&[KIND_UNIFORM, *delta as u8, *material]);
        }
        Chunk::Detail(d) => {
            out.push(KIND_DETAIL);
            write_plane(out, &d.delta.map(|v| v as u8));
            write_plane(out, &d.material);
        }
    }
}

/// Runs, unless plain samples are smaller: noise would triple in size as runs.
fn write_plane(out: &mut Vec<u8>, samples: &[u8; CHUNK_SAMPLES]) {
    let runs = encode_runs(samples);
    let body: &[u8] = if runs.len() < CHUNK_SAMPLES {
        &runs
    } else {
        samples
    };
    put_u32(out, len_u32(body.len()));
    out.extend_from_slice(body);
}

fn encode_runs(samples: &[u8; CHUNK_SAMPLES]) -> Vec<u8> {
    let mut out = Vec::new();
    for run in samples.chunk_by(|a, b| a == b) {
        put_u16(&mut out, run.len() as u16);
        out.push(run[0]);
    }
    out
}

pub(crate) fn read_volumes(body: &[u8]) -> Result<Vec<Volume>, FormatError> {
    read_with_budget(body, MAX_DETAIL_CHUNKS)
}

/// A volume sharing an order with a brush would be neither before nor after it,
/// so the field and the BSP could disagree about that brush.
pub(crate) fn check_orders(volumes: &[Volume], brushes: &[Brush]) -> Result<(), FormatError> {
    let mut orders: Vec<u32> = brushes.iter().map(Brush::order).collect();
    orders.sort_unstable();
    let tied = |v: &Volume| orders.binary_search(&v.order()).is_ok();
    if volumes.iter().any(tied) {
        return Err(FormatError::BadVolume);
    }
    Ok(())
}

fn read_with_budget(body: &[u8], mut budget: usize) -> Result<Vec<Volume>, FormatError> {
    let mut c = Cursor::new(body);
    let n = usize::try_from(c.u32()?).map_err(|_| FormatError::TooMany)?;
    let needed = n.checked_mul(VOLUME_HEAD).ok_or(FormatError::TooMany)?;
    if n > MAX_VOLUMES || needed > c.remaining() {
        return Err(FormatError::TooMany);
    }
    let mut out: Vec<Volume> = Vec::with_capacity(n);
    for _ in 0..n {
        let volume = read_volume(&mut c, &mut budget)?;
        if out.iter().any(|v| v.overlaps(&volume)) {
            return Err(FormatError::BadVolume);
        }
        out.push(volume);
    }
    if c.remaining() != 0 {
        return Err(FormatError::Trailing);
    }
    Ok(out)
}

fn read_volume(c: &mut Cursor, budget: &mut usize) -> Result<Volume, FormatError> {
    let order = c.u32()?;
    let op = match c.u8()? {
        0 => Op::Add,
        1 => Op::Subtract,
        other => return Err(FormatError::BadOp(other)),
    };
    let cell = CellSize::from_code(c.u8()?).ok_or(FormatError::BadVolume)?;
    let origin = [read_i32(c)?, read_i32(c)?, read_i32(c)?];
    let chunks = [c.u16()?, c.u16()?, c.u16()?];
    if chunks.iter().any(|&n| n == 0 || n > MAX_CHUNKS_PER_AXIS) {
        return Err(FormatError::BadVolume);
    }
    // The size is small now; the table is built only if the bytes could fill it.
    let count: usize = chunks.iter().map(|&n| usize::from(n)).product();
    if count.saturating_mul(CHUNK_MIN) > c.remaining() {
        return Err(FormatError::TooMany);
    }
    let mut volume =
        Volume::new(op, order, cell, origin, chunks).map_err(|_| FormatError::BadVolume)?;
    for slot in Arc::make_mut(&mut volume.table).iter_mut() {
        *slot = read_chunk(c, budget)?;
    }
    Ok(volume)
}

fn read_i32(c: &mut Cursor) -> Result<i32, FormatError> {
    Ok(c.u32()?.cast_signed())
}

fn read_chunk(c: &mut Cursor, budget: &mut usize) -> Result<Chunk, FormatError> {
    match c.u8()? {
        KIND_UNIFORM => {
            let delta = check_delta(c.u8()?)?;
            Ok(Chunk::Uniform {
                delta,
                material: c.u8()?,
            })
        }
        KIND_DETAIL => read_detail(c, budget),
        _ => Err(FormatError::BadVolume),
    }
}

/// Charges the budget first: this is the one allocation a file controls.
fn read_detail(c: &mut Cursor, budget: &mut usize) -> Result<Chunk, FormatError> {
    *budget = budget.checked_sub(1).ok_or(FormatError::TooMany)?;
    let mut deltas = [0u8; CHUNK_SAMPLES];
    read_plane(c, &mut deltas)?;
    let mut data = Arc::new(ChunkData {
        delta: [0; CHUNK_SAMPLES],
        material: [0; CHUNK_SAMPLES],
    });
    let d = Arc::make_mut(&mut data);
    for (slot, &raw) in d.delta.iter_mut().zip(&deltas) {
        *slot = check_delta(raw)?;
    }
    read_plane(c, &mut d.material)?;
    let mut chunk = Chunk::Detail(data);
    chunk.normalize();
    Ok(chunk)
}

fn check_delta(raw: u8) -> Result<i8, FormatError> {
    let delta = raw.cast_signed();
    if delta.unsigned_abs() > MAX_DELTA.unsigned_abs() {
        return Err(FormatError::BadVolume);
    }
    Ok(delta)
}

fn read_plane(c: &mut Cursor, out: &mut [u8; CHUNK_SAMPLES]) -> Result<(), FormatError> {
    let len = usize::try_from(c.u32()?).map_err(|_| FormatError::TooMany)?;
    let body = c.take(len)?;
    if len == CHUNK_SAMPLES {
        out.copy_from_slice(body);
        return Ok(());
    }
    if len % RUN_BYTES != 0 {
        return Err(FormatError::BadVolume);
    }
    let mut filled = 0usize;
    let (runs, _) = body.as_chunks::<RUN_BYTES>();
    for &[low, high, value] in runs {
        let count = usize::from(u16::from_le_bytes([low, high]));
        let end = filled + count;
        let target = out.get_mut(filled..end).ok_or(FormatError::BadVolume)?;
        if count == 0 {
            return Err(FormatError::BadVolume);
        }
        target.fill(value);
        filled = end;
    }
    if filled == CHUNK_SAMPLES {
        Ok(())
    } else {
        Err(FormatError::BadVolume)
    }
}

#[cfg(test)]
pub(super) fn read_for_test(body: &[u8], budget: usize) -> Result<Vec<Volume>, FormatError> {
    read_with_budget(body, budget)
}
