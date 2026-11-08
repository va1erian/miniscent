//! The level format's kinds section (id 6, version 7 and later).
//!
//! ```text
//! count u16, then per kind:
//!   name   len u8, bytes
//!   stats  hp u32, radius f32, speed f32, sight f32, attack range f32,
//!          damage u32, cooldown f32, lose f32
//!   text   len u32, a blueprint's bytes
//! ```
//!
//! Each blueprint goes through [`model::read`], so every limit a blueprint file
//! has holds here too. Names, stats and cost are judged by
//! [`Level::check_kinds`] once entities are known.
//!
//! [`Level::check_kinds`]: crate::entity::Level::check_kinds

use crate::format::FormatError;
use crate::format_wire::{Cursor, len_u32, put_f32, put_u16, put_u32};
use crate::kinds::{Kind, KindError, KindStats, MAX_KIND_NAME, MAX_LEVEL_KINDS};
use crate::model;

/// Smallest kind record: an empty name, the stats and an empty text.
const KIND_MIN: usize = 1 + 32 + 4;

pub(crate) fn write_kinds(kinds: &[Kind]) -> Vec<u8> {
    let mut out = Vec::new();
    // The reader refuses more than `MAX_LEVEL_KINDS`, so saturating is enough.
    put_u16(&mut out, u16::try_from(kinds.len()).unwrap_or(u16::MAX));
    for k in kinds {
        let name = &k.name.as_bytes()[..k.name.len().min(usize::from(u8::MAX))];
        out.push(name.len() as u8);
        out.extend_from_slice(name);
        put_stats(&mut out, &k.stats);
        let text = model::write(&k.blueprint);
        put_u32(&mut out, len_u32(text.len()));
        out.extend_from_slice(text.as_bytes());
    }
    out
}

fn put_stats(out: &mut Vec<u8>, s: &KindStats) {
    put_u32(out, s.hp);
    for v in [s.radius, s.speed, s.sight, s.attack_range] {
        put_f32(out, v);
    }
    put_u32(out, s.damage);
    put_f32(out, s.cooldown);
    put_f32(out, s.lose);
}

pub(crate) fn read_kinds(body: &[u8]) -> Result<Vec<Kind>, FormatError> {
    let mut c = Cursor::new(body);
    let count = usize::from(c.u16()?);
    let needed = count.checked_mul(KIND_MIN).ok_or(FormatError::TooMany)?;
    if count > MAX_LEVEL_KINDS || needed > c.remaining() {
        return Err(FormatError::Kinds(KindError::TooMany));
    }
    let kinds = (0..count)
        .map(|i| read_kind(&mut c, i))
        .collect::<Result<_, _>>()?;
    if c.remaining() != 0 {
        return Err(FormatError::Trailing);
    }
    Ok(kinds)
}

fn read_kind(c: &mut Cursor, index: usize) -> Result<Kind, FormatError> {
    let len = usize::from(c.u8()?);
    if len > MAX_KIND_NAME {
        return Err(FormatError::Kinds(KindError::BadName(index)));
    }
    let name = String::from_utf8(c.take(len)?.to_vec())
        .map_err(|_| FormatError::Kinds(KindError::BadName(index)))?;
    let stats = KindStats {
        hp: c.u32()?,
        radius: c.f32()?,
        speed: c.f32()?,
        sight: c.f32()?,
        attack_range: c.f32()?,
        damage: c.u32()?,
        cooldown: c.f32()?,
        lose: c.f32()?,
    };
    let text_len = usize::try_from(c.u32()?).map_err(|_| FormatError::TooLarge)?;
    if text_len > model::MAX_FILE {
        return Err(FormatError::Kinds(KindError::BadBlueprint(index)));
    }
    let blueprint = model::read(c.take(text_len)?)
        .map_err(|_| FormatError::Kinds(KindError::BadBlueprint(index)))?;
    Ok(Kind {
        name,
        stats,
        blueprint,
    })
}
