//! Names, tags and mover groups: what a level script refers to.
//!
//! These live beside the entities and brushes rather than on them, so
//! [`crate::Entity`] stays `Copy` and a level with no script costs nothing.
//! Names point from a script to the level, never from one level thing to
//! another, which is the rule that keeps logic out of level files
//! (docs/scripting.md).

use crate::brush::{Brush, Op};
use crate::entity::Level;
use std::borrow::Cow;
use std::collections::BTreeSet;

/// Longest name or tag, in bytes.
pub const MAX_NAME_BYTES: usize = 32;
/// Most tags on one entity or group.
pub const MAX_TAGS: usize = 8;
/// Most mover groups in a level.
pub const MAX_MOVER_GROUPS: usize = 1024;
/// Most brushes in one mover group.
pub const MAX_GROUP_BRUSHES: usize = 256;

/// The name and tags of one entity. An entity with neither has no label.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EntityLabel {
    /// Index into [`Level::entities`].
    pub entity: u32,
    pub name: Option<String>,
    pub tags: Vec<String>,
}

/// Brushes that move together. They stay out of the CSG, so a door is not
/// baked into the world.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MoverGroup {
    pub name: String,
    pub tags: Vec<String>,
    /// Indices into [`Level::brushes`].
    pub brushes: Vec<u32>,
}

/// Why a level's labels are unusable.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LabelError {
    /// A name or tag is empty, too long or not `[a-z0-9_]`.
    BadName,
    /// More tags, groups or group brushes than the caps allow.
    TooMany,
    /// An entity or brush index is out of range.
    BadIndex,
    /// A label has neither a name nor a tag.
    EmptyLabel,
    /// Two labels are for the same entity.
    DuplicateLabel,
    /// Two things share a name.
    DuplicateName,
    /// One thing lists a tag twice.
    DuplicateTag,
    /// A mover group has no brushes.
    EmptyGroup,
    /// A brush is in two mover groups, or twice in one.
    BrushInTwoGroups,
    /// A mover group holds a subtractive brush; a moving hole has no meaning.
    MoverNotAdd,
}

fn valid_name(s: &str) -> bool {
    let plain = |b: u8| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_';
    !s.is_empty() && s.len() <= MAX_NAME_BYTES && s.bytes().all(plain)
}

fn check_tags(tags: &[String]) -> Result<(), LabelError> {
    if tags.len() > MAX_TAGS {
        return Err(LabelError::TooMany);
    }
    let mut seen = BTreeSet::new();
    for tag in tags {
        if !valid_name(tag) {
            return Err(LabelError::BadName);
        }
        if !seen.insert(tag.as_str()) {
            return Err(LabelError::DuplicateTag);
        }
    }
    Ok(())
}

impl Level {
    /// Checks every label and group. The level reader enforces this, so a
    /// loaded level is always consistent.
    pub fn check_labels(&self) -> Result<(), LabelError> {
        let mut names = BTreeSet::new();
        self.check_entity_labels(&mut names)?;
        self.check_mover_groups(&mut names)
    }

    fn check_entity_labels<'a>(&'a self, names: &mut BTreeSet<&'a str>) -> Result<(), LabelError> {
        let mut labelled = BTreeSet::new();
        for label in &self.labels {
            if label.entity as usize >= self.entities.len() {
                return Err(LabelError::BadIndex);
            }
            if !labelled.insert(label.entity) {
                return Err(LabelError::DuplicateLabel);
            }
            if label.name.is_none() && label.tags.is_empty() {
                return Err(LabelError::EmptyLabel);
            }
            if let Some(name) = &label.name {
                check_name(name, names)?;
            }
            check_tags(&label.tags)?;
        }
        Ok(())
    }

    fn check_mover_groups<'a>(&'a self, names: &mut BTreeSet<&'a str>) -> Result<(), LabelError> {
        if self.mover_groups.len() > MAX_MOVER_GROUPS {
            return Err(LabelError::TooMany);
        }
        let mut used = BTreeSet::new();
        for group in &self.mover_groups {
            check_name(&group.name, names)?;
            check_tags(&group.tags)?;
            if group.brushes.is_empty() {
                return Err(LabelError::EmptyGroup);
            }
            if group.brushes.len() > MAX_GROUP_BRUSHES {
                return Err(LabelError::TooMany);
            }
            for &index in &group.brushes {
                let brush = self
                    .brushes
                    .get(index as usize)
                    .ok_or(LabelError::BadIndex)?;
                if brush.op() != Op::Add {
                    return Err(LabelError::MoverNotAdd);
                }
                if !used.insert(index) {
                    return Err(LabelError::BrushInTwoGroups);
                }
            }
        }
        Ok(())
    }

    /// The brushes that go through CSG: all of them except those in a mover
    /// group. Borrows when the level has no movers, the common case.
    pub fn static_brushes(&self) -> Cow<'_, [Brush]> {
        if self.mover_groups.is_empty() {
            return Cow::Borrowed(&self.brushes);
        }
        let moving: BTreeSet<u32> = self
            .mover_groups
            .iter()
            .flat_map(|g| g.brushes.iter().copied())
            .collect();
        let keep = |(i, b): (usize, &Brush)| {
            let moves = u32::try_from(i).is_ok_and(|i| moving.contains(&i));
            (!moves).then(|| b.clone())
        };
        Cow::Owned(self.brushes.iter().enumerate().filter_map(keep).collect())
    }
}

impl Level {
    /// The brushes the BSP is compiled from: the static ones, and the shell of
    /// each subtract volume. Faces are extracted from the same list. A shell
    /// that is not a valid brush cannot come from a volume the loader accepted.
    pub fn csg_brushes(&self) -> Cow<'_, [Brush]> {
        let brushes = self.static_brushes();
        let mut shells = self
            .volumes
            .iter()
            .filter_map(|v| v.shell()?.ok())
            .peekable();
        if shells.peek().is_none() {
            return brushes;
        }
        let mut all = brushes.into_owned();
        all.extend(shells);
        Cow::Owned(all)
    }
}

fn check_name<'a>(name: &'a str, names: &mut BTreeSet<&'a str>) -> Result<(), LabelError> {
    if !valid_name(name) {
        return Err(LabelError::BadName);
    }
    if !names.insert(name) {
        return Err(LabelError::DuplicateName);
    }
    Ok(())
}
