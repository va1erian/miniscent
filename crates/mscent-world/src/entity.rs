//! Game entities placed in a level (M2.6, reused by M6.2).
//!
//! Positions are `f32` world units: entities are runtime data, not CSG input.
//! [`Entity::is_valid`] is what the level reader enforces, so a loaded level
//! never carries NaN, infinite or non-positive-radius values.

use crate::brush::Brush;
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
}

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
        }
    }

    /// The entity's position.
    pub fn pos(&self) -> Vec3 {
        match *self {
            Entity::PlayerStart { pos, .. }
            | Entity::Light { pos, .. }
            | Entity::Enemy { pos, .. }
            | Entity::Goal { pos, .. } => pos,
        }
    }
}

/// A level's source data: brushes plus entities. Compiled data (BSP,
/// portals) is derived from the brushes on load rather than stored.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Level {
    pub brushes: Vec<Brush>,
    pub entities: Vec<Entity>,
}
