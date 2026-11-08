//! What a renderer derives once from a level and its materials.

use mscent_math::Vec3;
use mscent_world::entity::projector_dir;
use mscent_world::lightmap::{Cone, PointLight};
use mscent_world::{Entity, Level};

use super::{Actor, ActorKind, BuildConfig};
use crate::manifest::DEFAULT_GLOSS;
use crate::material::{MaterialId, MaterialTable};
use crate::mesh::{MeshLighting, PointLight as MeshLight};
use crate::specular::SpecTable;

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
                cone: None,
            }),
            Entity::Projector {
                pos,
                yaw,
                pitch,
                color,
                radius,
                intensity,
                inner,
                outer,
            } => Some(PointLight {
                position: pos,
                color: color * (intensity * gain),
                radius,
                // `is_valid` has held the angles, but a level built in code may not
                // have been checked: a bad cone is no light, not an all round one.
                cone: Some(Cone::new(projector_dir(yaw, pitch), inner, outer)?),
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
        // A mesh has no cone to test, so a projector would light it from behind;
        // it lights the level's surfaces only.
        points: lights
            .iter()
            .filter(|l| l.cone.is_none())
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
        .map(|i| {
            let gloss = table.get(MaterialId(i)).map_or(DEFAULT_GLOSS, |m| m.gloss);
            SpecTable::new(gloss)
        })
        .collect();
    tables.push(SpecTable::new(DEFAULT_GLOSS));
    tables
}

#[cfg(test)]
mod tests {
    use super::*;

    fn projector(inner: f32, outer: f32) -> Entity {
        Entity::Projector {
            pos: Vec3::new(0.0, 0.0, 5.0),
            yaw: 0.0,
            pitch: -core::f32::consts::FRAC_PI_2,
            color: Vec3::new(1.0, 1.0, 1.0),
            radius: 10.0,
            intensity: 2.0,
            inner,
            outer,
        }
    }

    fn level(entities: Vec<Entity>) -> Level {
        Level {
            entities,
            ..Default::default()
        }
    }

    #[test]
    fn a_projector_becomes_a_lamp_with_a_cone_pointing_where_it_is_aimed() {
        let lamps = lights_of(&level(vec![projector(0.1, 0.4)]), 1.0);
        assert_eq!(lamps.len(), 1);
        assert_eq!(lamps[0].color, Vec3::new(2.0, 2.0, 2.0));
        let cone = lamps[0].cone.expect("a cone");
        assert!((cone.dir() - Vec3::new(0.0, 0.0, -1.0)).length() < 1e-5);
    }

    #[test]
    fn a_projector_with_a_bad_cone_is_no_lamp_and_not_an_all_round_one() {
        // A level built in code is not checked by `is_valid`.
        assert!(lights_of(&level(vec![projector(0.5, 0.1)]), 1.0).is_empty());
    }

    #[test]
    fn models_are_not_lit_by_projectors() {
        let lamps = lights_of(&level(vec![projector(0.1, 0.4)]), 1.0);
        assert!(mesh_lighting(&lamps, Vec3::ZERO).points.is_empty());
    }
}
