//! The engine's errors in words for the status bar. Each says what is wrong and,
//! where there is something to do, what to do; none shows a Rust name. Entities
//! and brushes are numbered from 1, as the properties panel and `build::validate`
//! number them. The matches are exhaustive so a new error cannot slip through
//! as a code.

use mscent_game::LoadError;
use mscent_render::BuildError;
use mscent_world::format::FormatError;
use mscent_world::lightmap::LightmapError;
use mscent_world::{BrushError, CsgError, Entity, LabelError};

use crate::model::entity::entity_name;

/// What is wrong with the names, tags or mover groups.
pub fn label(e: LabelError) -> String {
    match e {
        LabelError::BadName => {
            "Names and tags are lower-case letters, digits and _, at most 32".into()
        }
        LabelError::TooMany => "Too many tags, mover groups or brushes in one group".into(),
        LabelError::BadIndex => "A name points at something that is not there".into(),
        LabelError::EmptyLabel => "A label has neither a name nor a tag".into(),
        LabelError::DuplicateLabel => "One thing has two labels".into(),
        LabelError::DuplicateName => "That name is already taken".into(),
        LabelError::DuplicateTag => "A tag is listed twice: list each tag once".into(),
        LabelError::EmptyGroup => "A mover has no brushes".into(),
        LabelError::BrushInTwoGroups => "A brush is in two movers: a brush moves with one".into(),
        LabelError::MoverNotAdd => {
            "A mover must be an add brush: a moving hole has no meaning".into()
        }
    }
}

/// Why a brush is not a solid.
pub fn brush(e: BrushError) -> &'static str {
    match e {
        BrushError::TooFewPlanes => "it has fewer than four sides",
        BrushError::TooManyPlanes => "it has too many sides",
        BrushError::NotFinite => "a coordinate is not a number",
        BrushError::Degenerate | BrushError::Empty => "it is flat or empty",
        BrushError::OutOfRange => "it reaches past the edge of the map (4096)",
        BrushError::Unbounded => "it is open on one side",
        BrushError::NonConvex => "it is not convex",
    }
}

/// Why a file is not a level the editor can read.
pub fn format(e: FormatError) -> String {
    match e {
        FormatError::BadMagic => "it is not a Miniscent level".into(),
        FormatError::UnsupportedVersion(v) => {
            format!("it was written by a newer Miniscent (format {v})")
        }
        FormatError::BadFlags | FormatError::Trailing | FormatError::BadOp(_) => {
            "it is damaged".into()
        }
        FormatError::BadEntityTag(_) => "it holds an entity this editor does not know".into(),
        FormatError::Truncated | FormatError::MissingSection(_) => "it is cut short".into(),
        FormatError::DuplicateSection(_) => "it is damaged: a part appears twice".into(),
        FormatError::TooLarge | FormatError::TooMany => "it holds more than a level may".into(),
        FormatError::BadEntity => "an entity has a value out of range".into(),
        FormatError::Brush(b) => format!("a brush is invalid: {}", brush(b)),
        FormatError::BadTexInfo => "a texture mapping is out of range".into(),
        FormatError::BadVolume => "a voxel volume is invalid or overlaps another".into(),
        FormatError::Labels(l) => label(l),
    }
}

/// Why the brushes do not compile into a map.
pub fn csg(e: CsgError) -> &'static str {
    match e {
        CsgError::TooManyBrushes => "the level has too many brushes",
        CsgError::TooDeep | CsgError::TooManyNodes => {
            "the level is too detailed: use fewer or simpler brushes"
        }
        CsgError::Invalid(_) => "the map came out broken, a bug in Miniscent",
    }
}

/// Why the level cannot be lit or drawn.
pub fn build(e: BuildError) -> String {
    match e {
        BuildError::Csg(c) => csg(c).into(),
        BuildError::Bake(l) => lighting(l).into(),
        BuildError::Materials(id) => format!("the material table is missing stock material {id}"),
    }
}

/// Why a bake failed.
pub fn lighting(e: LightmapError) -> &'static str {
    match e {
        LightmapError::DegenerateFace(_) | LightmapError::NonFiniteFace(_) => {
            "the lighting failed on a broken face: look for a sliver of a brush"
        }
        LightmapError::InvalidLight(_) => "a light has a value out of range",
        LightmapError::InvalidConfig => "the lighting settings are out of range",
        LightmapError::Atlas(_) => "the level has more surface than the lightmap can hold",
        LightmapError::Cancelled => "the lighting was cancelled",
    }
}

/// Why the level cannot be played, and the entity to blame when there is one,
/// by its index in `entities`.
pub fn load(e: LoadError, entities: &[Entity]) -> (String, Option<usize>) {
    let named = |i: usize| {
        let kind = entities.get(i).map_or("entity", entity_name);
        format!("{kind} (entity {})", i.saturating_add(1))
    };
    match e {
        LoadError::Csg(c) => (csg(c).into(), None),
        LoadError::NoPlayerStart => ("the level has no player start: place one".into(), None),
        LoadError::NoGoal => ("the level has no goal: place one".into(), None),
        LoadError::InvalidEntity(i) => (
            format!("{} has a value out of range", named(i)),
            Some(i),
        ),
        LoadError::SpawnInSolid(i) => (
            format!("{} is inside solid: move it into a room", named(i)),
            Some(i),
        ),
        LoadError::Labels(l) => (label(l), None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mscent_math::Vec3;

    #[test]
    fn a_spawn_in_solid_names_the_entity_from_one_and_blames_it() {
        let start = Entity::PlayerStart {
            pos: Vec3::ZERO,
            yaw: 0.0,
        };
        let (text, who) = load(LoadError::SpawnInSolid(1), &[start, start]);
        assert_eq!(text, "Player start (entity 2) is inside solid: move it into a room");
        assert_eq!(who, Some(1));
    }

    #[test]
    fn no_message_is_a_rust_name() {
        let texts = [
            label(LabelError::TooMany),
            label(LabelError::DuplicateTag),
            format(FormatError::BadEntity),
            format(FormatError::Brush(BrushError::Degenerate)),
            build(BuildError::Materials(3)),
            load(LoadError::InvalidEntity(usize::MAX), &[]).0,
        ];
        for text in texts {
            let camel = text
                .split_whitespace()
                .any(|w| w.chars().skip(1).any(char::is_uppercase));
            assert!(!camel, "{text}");
        }
    }
}
