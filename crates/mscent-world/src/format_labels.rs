//! The level format's labels section (id 4, version 3 and later).
//!
//! ```text
//! labels: count u32, then entity u32, name, tags
//! groups: count u32, then name, tags, brush count u32, brush index u32 each
//! name:   len u8, bytes; a label's len 0 means it has no name
//! tags:   count u8, then a name each
//! ```
//!
//! Bytes are only checked to be UTF-8 here; [`Level::check_labels`] judges
//! the content once brushes and entities are both known.
//!
//! [`Level::check_labels`]: crate::entity::Level::check_labels

use crate::format::{FormatError, read_count};
use crate::format_wire::{Cursor, len_u32, put_u32};
use crate::labels::{EntityLabel, LabelError, MAX_GROUP_BRUSHES, MAX_MOVER_GROUPS, MoverGroup};

/// Most labels: one per entity at most, so this is the entity cap.
const MAX_LABELS: usize = crate::format::MAX_ENTITIES;
/// Smallest label record: entity, empty name, no tags.
const LABEL_MIN: usize = 4 + 1 + 1;
/// Smallest group record: empty name, no tags, no brushes.
const GROUP_MIN: usize = 1 + 1 + 4;

/// Writes a string with a saturated length. Valid names are short; an invalid
/// one is cut at the cap and then rejected by the reader as too long.
fn put_name(out: &mut Vec<u8>, name: &str) {
    let bytes = &name.as_bytes()[..name.len().min(usize::from(u8::MAX))];
    out.push(bytes.len() as u8);
    out.extend_from_slice(bytes);
}

fn put_tags(out: &mut Vec<u8>, tags: &[String]) {
    let tags = &tags[..tags.len().min(usize::from(u8::MAX))];
    out.push(tags.len() as u8);
    for tag in tags {
        put_name(out, tag);
    }
}

pub(crate) fn write_labels(labels: &[EntityLabel], groups: &[MoverGroup]) -> Vec<u8> {
    let mut out = Vec::new();
    put_u32(&mut out, len_u32(labels.len()));
    for label in labels {
        put_u32(&mut out, label.entity);
        put_name(&mut out, label.name.as_deref().unwrap_or(""));
        put_tags(&mut out, &label.tags);
    }
    put_u32(&mut out, len_u32(groups.len()));
    for group in groups {
        put_name(&mut out, &group.name);
        put_tags(&mut out, &group.tags);
        put_u32(&mut out, len_u32(group.brushes.len()));
        for &brush in &group.brushes {
            put_u32(&mut out, brush);
        }
    }
    out
}

fn read_name(c: &mut Cursor) -> Result<String, FormatError> {
    let len = usize::from(c.u8()?);
    let bytes = c.take(len)?;
    String::from_utf8(bytes.to_vec()).map_err(|_| FormatError::Labels(LabelError::BadName))
}

fn read_tags(c: &mut Cursor) -> Result<Vec<String>, FormatError> {
    let count = c.u8()?;
    (0..count).map(|_| read_name(c)).collect()
}

fn read_label(c: &mut Cursor) -> Result<EntityLabel, FormatError> {
    let entity = c.u32()?;
    let name = read_name(c)?;
    Ok(EntityLabel {
        entity,
        name: (!name.is_empty()).then_some(name),
        tags: read_tags(c)?,
    })
}

fn read_group(c: &mut Cursor) -> Result<MoverGroup, FormatError> {
    let name = read_name(c)?;
    let tags = read_tags(c)?;
    let count = read_count(c, MAX_GROUP_BRUSHES, 4)?;
    let brushes = (0..count).map(|_| c.u32()).collect::<Result<_, _>>()?;
    Ok(MoverGroup {
        name,
        tags,
        brushes,
    })
}

pub(crate) fn read_labels(body: &[u8]) -> Result<(Vec<EntityLabel>, Vec<MoverGroup>), FormatError> {
    let mut c = Cursor::new(body);
    let count = read_count(&mut c, MAX_LABELS, LABEL_MIN)?;
    let labels = (0..count)
        .map(|_| read_label(&mut c))
        .collect::<Result<_, _>>()?;
    let count = read_count(&mut c, MAX_MOVER_GROUPS, GROUP_MIN)?;
    let groups = (0..count)
        .map(|_| read_group(&mut c))
        .collect::<Result<_, _>>()?;
    if c.remaining() != 0 {
        return Err(FormatError::Trailing);
    }
    Ok((labels, groups))
}
