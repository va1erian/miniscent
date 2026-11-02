//! What a renderer derives once from a level and its materials.

use mscent_math::Vec3;
use mscent_world::lightmap::PointLight;
use mscent_world::{Entity, Level};

use super::{Actor, ActorKind, BuildConfig};
use crate::mesh::{MeshLighting, PointLight as MeshLight};
use crate::specular::SpecTable;
use crate::texture::{MaterialId, MaterialTable};

pub(super) fn lights_of(level: &Level, gain: f32) -> Vec<PointLight> {
    level
        .entities
        .iter()
        .filter_map(|e| match *e {
            Entity::Light {
                pos,
                color,
                radius,
                intensity,
            } => Some(PointLight {
                position: pos,
                color: color * (intensity * gain),
                radius,
            }),
            _ => None,
        })
        .collect()
}

/// `cfg` with the fog of the level's world entity, if it has one. Fog does
/// not affect the bake.
pub(super) fn with_level_fog(cfg: &BuildConfig, level: &Level) -> BuildConfig {
    match level.fog() {
        Some((fog_start, fog_end)) => BuildConfig {
            fog_start,
            fog_end,
            ..*cfg
        },
        None => *cfg,
    }
}

pub(super) fn actors_of(level: &Level) -> Vec<Actor> {
    let actor = |kind, pos| Actor {
        kind,
        pos,
        yaw: 0.0,
    };
    level
        .entities
        .iter()
        .filter_map(|e| match *e {
            Entity::Enemy { pos, .. } => Some(actor(ActorKind::Enemy, pos)),
            Entity::Goal { pos, .. } => Some(actor(ActorKind::Goal, pos)),
            _ => None,
        })
        .collect()
}

/// Entities cannot sample the lightmap (it is per face), so they are lit by
/// the same point lights without shadows plus a little ambient.
pub(super) fn mesh_lighting(lights: &[PointLight], ambient: Vec3) -> MeshLighting {
    MeshLighting {
        ambient: 0.08 + (ambient.x + ambient.y + ambient.z) / 3.0,
        sun_dir: Vec3::new(0.3, -0.4, 0.85)
            .normalized()
            .unwrap_or(Vec3::ZERO),
        sun: 0.25,
        points: lights
            .iter()
            .map(|l| MeshLight {
                pos: l.position,
                radius: l.radius,
                intensity: (l.color.x + l.color.y + l.color.z) / 3.0,
            })
            .collect(),
    }
}

/// One specular table per material id, plus a last one for unknown ids.
pub(super) fn spec_tables(table: &MaterialTable) -> Vec<SpecTable> {
    let mut tables: Vec<SpecTable> = (0u16..)
        .take(table.len())
        .map(|i| SpecTable::new(table.get(MaterialId(i)).map_or(40, |m| m.gloss)))
        .collect();
    tables.push(SpecTable::new(40));
    tables
}
