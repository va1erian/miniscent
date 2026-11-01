//! Versioned binary level file (M2.6).
//!
//! Layout, all little-endian:
//!
//! ```text
//! magic   "MSLV"           4 bytes
//! version u16              currently 1
//! flags   u16              reserved, must be 0
//! count   u32              number of sections (<= MAX_SECTIONS)
//! section*: id u32, len u32, payload[len]
//! ```
//!
//! Sections: `1` brushes (required), `2` entities (required); `3` is
//! reserved for compiled data, which the loader skips because it recompiles
//! from brushes. Unknown section ids are skipped for forward compatibility.
//! Brushes are stored as raw planes and rebuilt through [`Brush::new`], so a
//! hostile file cannot smuggle in an invalid brush. Counts are checked
//! against the bytes actually present before anything is allocated.

use crate::brush::{Brush, BrushError, MAX_PLANES, Op};
use crate::entity::{Entity, Level};
use crate::format_wire::{Cursor, len_u32, put_f32, put_f64, put_u16, put_u32, put_vec3};
use mscent_math::{DPlane, Vec3d};

pub const MAGIC: [u8; 4] = *b"MSLV";
pub const VERSION: u16 = 1;
pub const MAX_FILE_BYTES: usize = 64 << 20;
pub const MAX_BRUSHES: usize = 65_536;
pub const MAX_ENTITIES: usize = 65_536;
pub const MAX_SECTIONS: u32 = 16;

const SEC_BRUSHES: u32 = 1;
const SEC_ENTITIES: u32 = 2;

/// Smallest brush record: op, order, material, plane count.
const BRUSH_HEAD: usize = 1 + 4 + 2 + 4;
const PLANE_BYTES: usize = 32;
/// Smallest entity record: a tag plus a position.
const ENTITY_MIN: usize = 1 + 12;

/// Why a level file was rejected.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FormatError {
    BadMagic,
    UnsupportedVersion(u16),
    /// Reserved flag bits were set.
    BadFlags,
    /// The data ended before a field or section did.
    Truncated,
    /// File larger than [`MAX_FILE_BYTES`].
    TooLarge,
    /// A count exceeds its cap or the bytes available.
    TooMany,
    DuplicateSection(u32),
    MissingSection(u32),
    /// Bytes left over after the last section or inside a section.
    Trailing,
    BadOp(u8),
    BadEntityTag(u8),
    /// An entity field is NaN, infinite or out of range.
    BadEntity,
    Brush(BrushError),
}

/// Serialises a level (source data only).
pub fn write(level: &Level) -> Vec<u8> {
    let brushes = write_brushes(&level.brushes);
    let entities = write_entities(&level.entities);
    let mut out = Vec::with_capacity(32 + brushes.len() + entities.len());
    out.extend_from_slice(&MAGIC);
    put_u16(&mut out, VERSION);
    put_u16(&mut out, 0);
    put_u32(&mut out, 2);
    for (id, body) in [(SEC_BRUSHES, brushes), (SEC_ENTITIES, entities)] {
        put_u32(&mut out, id);
        put_u32(&mut out, len_u32(body.len()));
        out.extend_from_slice(&body);
    }
    out
}

fn write_brushes(brushes: &[Brush]) -> Vec<u8> {
    let mut out = Vec::new();
    put_u32(&mut out, len_u32(brushes.len()));
    for b in brushes {
        out.push(match b.op() {
            Op::Add => 0,
            Op::Subtract => 1,
        });
        put_u32(&mut out, b.order());
        put_u16(&mut out, b.material());
        put_u32(&mut out, len_u32(b.planes().len()));
        for p in b.planes() {
            for v in [p.normal.x, p.normal.y, p.normal.z, p.dist] {
                put_f64(&mut out, v);
            }
        }
    }
    out
}

fn write_entities(entities: &[Entity]) -> Vec<u8> {
    let mut out = Vec::new();
    put_u32(&mut out, len_u32(entities.len()));
    for e in entities {
        out.push(match e {
            Entity::PlayerStart { .. } => 0,
            Entity::Light { .. } => 1,
            Entity::Enemy { .. } => 2,
            Entity::Goal { .. } => 3,
        });
        put_vec3(&mut out, e.pos());
        match *e {
            Entity::PlayerStart { yaw, .. } => put_f32(&mut out, yaw),
            Entity::Light {
                color,
                radius,
                intensity,
                ..
            } => {
                put_vec3(&mut out, color);
                put_f32(&mut out, radius);
                put_f32(&mut out, intensity);
            }
            Entity::Enemy { kind, .. } => out.push(kind),
            Entity::Goal { radius, .. } => put_f32(&mut out, radius),
        }
    }
    out
}

/// Parses a level file. Never panics; any malformed input is an `Err`.
pub fn read(data: &[u8]) -> Result<Level, FormatError> {
    if data.len() > MAX_FILE_BYTES {
        return Err(FormatError::TooLarge);
    }
    let mut c = Cursor::new(data);
    if c.take(4)? != MAGIC {
        return Err(FormatError::BadMagic);
    }
    let version = c.u16()?;
    if version != VERSION {
        return Err(FormatError::UnsupportedVersion(version));
    }
    if c.u16()? != 0 {
        return Err(FormatError::BadFlags);
    }
    let count = c.u32()?;
    if count > MAX_SECTIONS {
        return Err(FormatError::TooMany);
    }
    let (mut brushes, mut entities) = (None, None);
    for _ in 0..count {
        let id = c.u32()?;
        let len = usize::try_from(c.u32()?).map_err(|_| FormatError::TooLarge)?;
        let body = c.take(len)?;
        match id {
            SEC_BRUSHES => store(&mut brushes, id, read_brushes(body))?,
            SEC_ENTITIES => store(&mut entities, id, read_entities(body))?,
            _ => {}
        }
    }
    if c.remaining() != 0 {
        return Err(FormatError::Trailing);
    }
    Ok(Level {
        brushes: brushes.ok_or(FormatError::MissingSection(SEC_BRUSHES))?,
        entities: entities.ok_or(FormatError::MissingSection(SEC_ENTITIES))?,
    })
}

fn store<T>(
    slot: &mut Option<T>,
    id: u32,
    parsed: Result<T, FormatError>,
) -> Result<(), FormatError> {
    if slot.is_some() {
        return Err(FormatError::DuplicateSection(id));
    }
    *slot = Some(parsed?);
    Ok(())
}

fn read_count(c: &mut Cursor, cap: usize, min_record: usize) -> Result<usize, FormatError> {
    let n = usize::try_from(c.u32()?).map_err(|_| FormatError::TooMany)?;
    // Reject counts the remaining bytes cannot possibly hold, before
    // allocating for them.
    let needed = n.checked_mul(min_record).ok_or(FormatError::TooMany)?;
    if n > cap || needed > c.remaining() {
        return Err(FormatError::TooMany);
    }
    Ok(n)
}

fn read_brushes(body: &[u8]) -> Result<Vec<Brush>, FormatError> {
    let mut c = Cursor::new(body);
    let n = read_count(&mut c, MAX_BRUSHES, BRUSH_HEAD)?;
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        out.push(read_brush(&mut c)?);
    }
    if c.remaining() != 0 {
        return Err(FormatError::Trailing);
    }
    Ok(out)
}

fn read_brush(c: &mut Cursor) -> Result<Brush, FormatError> {
    let op = match c.u8()? {
        0 => Op::Add,
        1 => Op::Subtract,
        other => return Err(FormatError::BadOp(other)),
    };
    let order = c.u32()?;
    let material = c.u16()?;
    let np = usize::try_from(c.u32()?).map_err(|_| FormatError::TooMany)?;
    let bytes = np.checked_mul(PLANE_BYTES).ok_or(FormatError::TooMany)?;
    if np > MAX_PLANES || bytes > c.remaining() {
        return Err(FormatError::TooMany);
    }
    let mut planes = Vec::with_capacity(np);
    for _ in 0..np {
        let n = Vec3d::new(c.f64()?, c.f64()?, c.f64()?);
        planes.push(DPlane::new(n, c.f64()?));
    }
    Brush::new(&planes, op, order)
        .map(|b| b.with_material(material))
        .map_err(FormatError::Brush)
}

fn read_entities(body: &[u8]) -> Result<Vec<Entity>, FormatError> {
    let mut c = Cursor::new(body);
    let n = read_count(&mut c, MAX_ENTITIES, ENTITY_MIN)?;
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        let e = read_entity(&mut c)?;
        if !e.is_valid() {
            return Err(FormatError::BadEntity);
        }
        out.push(e);
    }
    if c.remaining() != 0 {
        return Err(FormatError::Trailing);
    }
    Ok(out)
}

fn read_entity(c: &mut Cursor) -> Result<Entity, FormatError> {
    let tag = c.u8()?;
    let pos = c.vec3()?;
    Ok(match tag {
        0 => Entity::PlayerStart { pos, yaw: c.f32()? },
        1 => Entity::Light {
            pos,
            color: c.vec3()?,
            radius: c.f32()?,
            intensity: c.f32()?,
        },
        2 => Entity::Enemy { pos, kind: c.u8()? },
        3 => Entity::Goal {
            pos,
            radius: c.f32()?,
        },
        other => return Err(FormatError::BadEntityTag(other)),
    })
}
