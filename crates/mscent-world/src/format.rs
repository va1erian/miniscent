//! Versioned binary level file (M2.6).
//!
//! Layout, all little-endian:
//!
//! ```text
//! magic   "MSLV"           4 bytes
//! version u16              currently 8 (1 to 7 are still read)
//! flags   u16              reserved, must be 0
//! count   u32              number of sections (<= MAX_SECTIONS)
//! section*: id u32, len u32, payload[len]
//! ```
//!
//! Sections: `1` brushes (required), `2` entities (required); `3` is
//! reserved for compiled data, which the loader skips because it recompiles
//! from brushes; `4` labels (`format_labels.rs`, version 3, written only when
//! there are any); `5` voxel volumes (`voxel/wire.rs`, version 4, likewise); `6`
//! enemy kinds (`format_kinds.rs`, version 7, likewise: none means the stock table).
//! Unknown section ids are skipped for forward compatibility. Entity tag `7`, a
//! vista layer, is version 5; an older file with it is rejected. Entity tag `9`,
//! a scale, is version 8.
//! Brushes are stored as raw planes and rebuilt through [`Brush::new`], so a
//! hostile file cannot smuggle in an invalid brush. Version 2 follows each
//! brush's planes with one TexInfo record per plane (`texinfo.rs`, 47 bytes);
//! version 1 has none and every plane gets the default mapping for its
//! normal, with the brush's material. Counts are checked
//! against the bytes actually present before anything is allocated.

use crate::brush::{Brush, BrushError, MAX_PLANES, Op};
use crate::entity::{Entity, Level};
use crate::format_kinds::{read_kinds, write_kinds};
use crate::format_labels::{read_labels, write_labels};
use crate::format_wire::{
    Cursor, TEXINFO_BYTES, len_u32, put_f32, put_f64, put_texinfo, put_u16, put_u32, put_vec3,
};
use crate::kinds::KindError;
use crate::labels::LabelError;
use crate::texinfo::TexInfo;
use crate::voxel::wire::{check_orders, read_volumes, write_volumes};
use mscent_math::{DPlane, Vec3d};

pub const MAGIC: [u8; 4] = *b"MSLV";
pub const VERSION: u16 = 8;
/// Oldest version [`read`] still accepts.
const MIN_VERSION: u16 = 1;
pub const MAX_FILE_BYTES: usize = 64 << 20;
pub const MAX_BRUSHES: usize = 65_536;
pub const MAX_ENTITIES: usize = 65_536;
pub const MAX_SECTIONS: u32 = 16;

const SEC_BRUSHES: u32 = 1;
const SEC_ENTITIES: u32 = 2;
const SEC_LABELS: u32 = 4;
const SEC_VOLUMES: u32 = 5;
const SEC_KINDS: u32 = 6;
/// First version whose labels section the reader understands.
const LABELS_VERSION: u16 = 3;
/// First version whose volumes section the reader understands.
const VOLUMES_VERSION: u16 = 4;
/// First version whose entities may be vista layers (tag 7).
const VISTA_VERSION: u16 = 5;
/// First version with [`Entity::Projector`], entity tag 8.
const PROJECTOR_VERSION: u16 = 6;
/// First version whose kinds section the reader understands.
const KINDS_VERSION: u16 = 7;
/// First version with [`Entity::Scale`], entity tag 9.
const SCALE_VERSION: u16 = 8;

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
    /// A texture mapping field is non-finite or out of range.
    BadTexInfo,
    /// A voxel volume has a bad field, a forged plane or overlaps another.
    BadVolume,
    /// Names, tags or mover groups are inconsistent.
    Labels(LabelError),
    /// Enemy kinds are malformed, too costly, or an enemy names none of them.
    Kinds(KindError),
}

/// Serialises a level (source data only).
pub fn write(level: &Level) -> Vec<u8> {
    let brushes = write_brushes(&level.brushes);
    let entities = write_entities(&level.entities);
    let mut sections = vec![(SEC_BRUSHES, brushes), (SEC_ENTITIES, entities)];
    if !level.labels.is_empty() || !level.mover_groups.is_empty() {
        sections.push((SEC_LABELS, write_labels(&level.labels, &level.mover_groups)));
    }
    if !level.volumes.is_empty() {
        sections.push((SEC_VOLUMES, write_volumes(&level.volumes)));
    }
    if !level.kinds.is_empty() {
        sections.push((SEC_KINDS, write_kinds(&level.kinds)));
    }
    let bodies: usize = sections.iter().map(|(_, body)| body.len()).sum();
    let mut out = Vec::with_capacity(32 + bodies);
    out.extend_from_slice(&MAGIC);
    put_u16(&mut out, VERSION);
    put_u16(&mut out, 0);
    put_u32(&mut out, len_u32(sections.len()));
    for (id, body) in sections {
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
        for t in b.texinfos() {
            put_texinfo(&mut out, t);
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
            Entity::World { .. } => 4,
            Entity::Marker { .. } => 5,
            Entity::Zone { .. } => 6,
            Entity::VistaLayer { .. } => 7,
            Entity::Projector { .. } => 8,
            Entity::Scale { .. } => 9,
        });
        put_vec3(&mut out, e.pos());
        match *e {
            Entity::PlayerStart { yaw, .. } | Entity::Marker { yaw, .. } => put_f32(&mut out, yaw),
            Entity::Zone { half, .. } => put_vec3(&mut out, half),
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
            Entity::Projector {
                yaw,
                pitch,
                color,
                radius,
                intensity,
                inner,
                outer,
                ..
            } => {
                put_vec3(&mut out, color);
                for v in [yaw, pitch, radius, intensity, inner, outer] {
                    put_f32(&mut out, v);
                }
            }
            Entity::Enemy { kind, .. } => out.push(kind),
            Entity::Goal { radius, .. } | Entity::Scale { ship: radius, .. } => {
                put_f32(&mut out, radius);
            }
            Entity::World {
                fog_start, fog_end, ..
            } => {
                put_f32(&mut out, fog_start);
                put_f32(&mut out, fog_end);
            }
            Entity::VistaLayer {
                centre,
                slot,
                layer,
                parallax,
                spin,
                pan,
                fog_start,
                fog_end,
                fog_color,
                ..
            } => {
                put_vec3(&mut out, centre);
                out.extend_from_slice(&[slot, layer]);
                put_f32(&mut out, parallax);
                put_vec3(&mut out, spin);
                pan.iter().for_each(|&v| put_f32(&mut out, v));
                put_f32(&mut out, fog_start);
                put_f32(&mut out, fog_end);
                put_u32(&mut out, fog_color);
            }
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
    if !(MIN_VERSION..=VERSION).contains(&version) {
        return Err(FormatError::UnsupportedVersion(version));
    }
    if c.u16()? != 0 {
        return Err(FormatError::BadFlags);
    }
    let count = c.u32()?;
    if count > MAX_SECTIONS {
        return Err(FormatError::TooMany);
    }
    let (mut brushes, mut entities, mut labels, mut volumes) = (None, None, None, None);
    let mut kinds = None;
    for _ in 0..count {
        let id = c.u32()?;
        let len = usize::try_from(c.u32()?).map_err(|_| FormatError::TooLarge)?;
        let body = c.take(len)?;
        match id {
            SEC_BRUSHES => store(&mut brushes, id, read_brushes(body, version))?,
            SEC_ENTITIES => store(&mut entities, id, read_entities(body, version))?,
            SEC_LABELS if version >= LABELS_VERSION => store(&mut labels, id, read_labels(body))?,
            SEC_VOLUMES if version >= VOLUMES_VERSION => {
                store(&mut volumes, id, read_volumes(body))?;
            }
            SEC_KINDS if version >= KINDS_VERSION => store(&mut kinds, id, read_kinds(body))?,
            _ => {}
        }
    }
    if c.remaining() != 0 {
        return Err(FormatError::Trailing);
    }
    let (labels, mover_groups) = labels.unwrap_or_default();
    let brushes = brushes.ok_or(FormatError::MissingSection(SEC_BRUSHES))?;
    let volumes = volumes.unwrap_or_default();
    check_orders(&volumes, &brushes)?;
    let level = Level {
        brushes,
        entities: entities.ok_or(FormatError::MissingSection(SEC_ENTITIES))?,
        volumes,
        labels,
        mover_groups,
        kinds: kinds.unwrap_or_default(),
    };
    // Labels and kinds refer to entities, so this waits until all are read.
    level.check_labels().map_err(FormatError::Labels)?;
    level.check_kinds().map_err(FormatError::Kinds)?;
    Ok(level)
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

pub(crate) fn read_count(
    c: &mut Cursor,
    cap: usize,
    min_record: usize,
) -> Result<usize, FormatError> {
    let n = usize::try_from(c.u32()?).map_err(|_| FormatError::TooMany)?;
    // Reject counts the remaining bytes cannot possibly hold, before
    // allocating for them.
    let needed = n.checked_mul(min_record).ok_or(FormatError::TooMany)?;
    if n > cap || needed > c.remaining() {
        return Err(FormatError::TooMany);
    }
    Ok(n)
}

fn read_brushes(body: &[u8], version: u16) -> Result<Vec<Brush>, FormatError> {
    let mut c = Cursor::new(body);
    let n = read_count(&mut c, MAX_BRUSHES, BRUSH_HEAD)?;
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        out.push(read_brush(&mut c, version)?);
    }
    if c.remaining() != 0 {
        return Err(FormatError::Trailing);
    }
    Ok(out)
}

fn read_brush(c: &mut Cursor, version: u16) -> Result<Brush, FormatError> {
    let op = match c.u8()? {
        0 => Op::Add,
        1 => Op::Subtract,
        other => return Err(FormatError::BadOp(other)),
    };
    let order = c.u32()?;
    let material = c.u16()?;
    let np = usize::try_from(c.u32()?).map_err(|_| FormatError::TooMany)?;
    let per_plane = PLANE_BYTES + if version >= 2 { TEXINFO_BYTES } else { 0 };
    let bytes = np.checked_mul(per_plane).ok_or(FormatError::TooMany)?;
    if np > MAX_PLANES || bytes > c.remaining() {
        return Err(FormatError::TooMany);
    }
    let mut planes = Vec::with_capacity(np);
    for _ in 0..np {
        let n = Vec3d::new(c.f64()?, c.f64()?, c.f64()?);
        planes.push(DPlane::new(n, c.f64()?));
    }
    let infos = if version >= 2 {
        (0..np)
            .map(|_| c.texinfo())
            .collect::<Result<Vec<_>, _>>()?
    } else {
        let default = |p: &DPlane| TexInfo {
            material,
            ..TexInfo::default_for_normal(p.normal)
        };
        planes.iter().map(default).collect()
    };
    Brush::new_textured(&planes, &infos, op, order).map_err(FormatError::Brush)
}

fn read_entities(body: &[u8], version: u16) -> Result<Vec<Entity>, FormatError> {
    let mut c = Cursor::new(body);
    let n = read_count(&mut c, MAX_ENTITIES, ENTITY_MIN)?;
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        let e = read_entity(&mut c, version)?;
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

fn read_entity(c: &mut Cursor, version: u16) -> Result<Entity, FormatError> {
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
        4 => Entity::World {
            pos,
            fog_start: c.f32()?,
            fog_end: c.f32()?,
        },
        5 => Entity::Marker { pos, yaw: c.f32()? },
        6 => Entity::Zone {
            pos,
            half: c.vec3()?,
        },
        8 if version >= PROJECTOR_VERSION => Entity::Projector {
            pos,
            color: c.vec3()?,
            yaw: c.f32()?,
            pitch: c.f32()?,
            radius: c.f32()?,
            intensity: c.f32()?,
            inner: c.f32()?,
            outer: c.f32()?,
        },
        7 if version >= VISTA_VERSION => Entity::VistaLayer {
            pos,
            centre: c.vec3()?,
            slot: c.u8()?,
            layer: c.u8()?,
            parallax: c.f32()?,
            spin: c.vec3()?,
            pan: [c.f32()?, c.f32()?],
            fog_start: c.f32()?,
            fog_end: c.f32()?,
            fog_color: c.u32()?,
        },
        9 if version >= SCALE_VERSION => Entity::Scale {
            pos,
            ship: c.f32()?,
        },
        other => return Err(FormatError::BadEntityTag(other)),
    })
}
