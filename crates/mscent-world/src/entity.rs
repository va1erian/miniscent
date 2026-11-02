//! Game entities placed in a level (M2.6, reused by M6.2).
//!
//! Positions are `f32` world units: entities are runtime data, not CSG input.
//! [`Entity::is_valid`] is what the level reader enforces, so a loaded level
//! never carries NaN, infinite or non-positive-radius values.

use crate::brush::Brush;
use crate::voxel::Volume;
use mscent_math::Vec3;

/// Largest absolute coordinate, radius or intensity accepted for an entity.
pub const MAX_ENTITY_COORD: f32 = 1.0e6;

/// One placed game element.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Entity {
    /// Where the player spawns, facing `yaw` radians.
    PlayerStart { pos: Vec3, yaw: f32 },
    /// Point light; `color` is linear RGB in `0..=1` per channel.
    Light {
        pos: Vec3,
        color: Vec3,
        radius: f32,
        intensity: f32,
    },
    /// Enemy spawn; `kind` indexes the game's enemy table.
    Enemy { pos: Vec3, kind: u8 },
    /// Reaching within `radius` of `pos` wins the level.
    Goal { pos: Vec3, radius: f32 },
    /// Per-level look settings; the first one in a level counts. `pos` is
    /// only where the editor draws it. Fog starts at view depth `fog_start`
    /// and is total at `fog_end`.
    World {
        pos: Vec3,
        fog_start: f32,
        fog_end: f32,
    },
}

/// Largest fog distance a level may ask for.
pub const MAX_FOG_DEPTH: f32 = 1000.0;

fn coord_ok(p: Vec3) -> bool {
    p.is_finite()
        && p.x.abs() <= MAX_ENTITY_COORD
        && p.y.abs() <= MAX_ENTITY_COORD
        && p.z.abs() <= MAX_ENTITY_COORD
}

fn radius_ok(r: f32) -> bool {
    r.is_finite() && r > 0.0 && r <= MAX_ENTITY_COORD
}

fn unit_ok(v: f32) -> bool {
    (0.0..=1.0).contains(&v)
}

impl Entity {
    /// Whether every field is finite and in range.
    pub fn is_valid(&self) -> bool {
        match *self {
            Entity::PlayerStart { pos, yaw } => coord_ok(pos) && yaw.is_finite(),
            Entity::Light {
                pos,
                color,
                radius,
                intensity,
            } => {
                coord_ok(pos)
                    && unit_ok(color.x)
                    && unit_ok(color.y)
                    && unit_ok(color.z)
                    && radius_ok(radius)
                    && (0.0..=MAX_ENTITY_COORD).contains(&intensity)
            }
            Entity::Enemy { pos, .. } => coord_ok(pos),
            Entity::Goal { pos, radius } => coord_ok(pos) && radius_ok(radius),
            Entity::World {
                pos,
                fog_start,
                fog_end,
            } => {
                coord_ok(pos)
                    && fog_start.is_finite()
                    && fog_start >= 0.0
                    && fog_end > fog_start
                    && fog_end <= MAX_FOG_DEPTH
            }
        }
    }

    /// The entity's position.
    pub fn pos(&self) -> Vec3 {
        match *self {
            Entity::PlayerStart { pos, .. }
            | Entity::Light { pos, .. }
            | Entity::Enemy { pos, .. }
            | Entity::Goal { pos, .. }
            | Entity::World { pos, .. } => pos,
        }
    }
}

impl Level {
    /// The fog range `(start, end)` of the level's first [`Entity::World`].
    pub fn fog(&self) -> Option<(f32, f32)> {
        self.entities.iter().find_map(|e| match *e {
            Entity::World {
                fog_start, fog_end, ..
            } => Some((fog_start, fog_end)),
            _ => None,
        })
    }
}

/// A level's source data: brushes plus entities. Compiled data (BSP,
/// portals) is derived from the brushes on load rather than stored.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Level {
    pub brushes: Vec<Brush>,
    pub entities: Vec<Entity>,
    /// Voxel volumes, none of which overlap. Their CSG order shares `Brush::order`.
    pub volumes: Vec<Volume>,
}
