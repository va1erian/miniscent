//! World instantiation (M6.2): turns a [`Level`] into the immutable data a
//! game runs on. Dynamic state (player, enemies) is created from it by
//! [`crate::Game`], so a restart never has to reload the level.

use mscent_math::Vec3;
use mscent_world::{Bsp, CsgError, Entity, Level, compile};

/// A placed point light, handed to the renderer.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LightSpec {
    pub pos: Vec3,
    pub color: Vec3,
    pub radius: f32,
    pub intensity: f32,
}

/// A win zone.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Goal {
    pub pos: Vec3,
    pub radius: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EnemySpawn {
    pub pos: Vec3,
    pub kind: u8,
}

/// Why a level cannot be played.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoadError {
    Csg(CsgError),
    NoPlayerStart,
    NoGoal,
    /// Entity at this index in the level fails validation.
    InvalidEntity(usize),
    /// Player start or enemy at this entity index is inside solid rock.
    SpawnInSolid(usize),
}

/// Immutable per-level game data.
#[derive(Clone, Debug, PartialEq)]
pub struct Arena {
    pub bsp: Bsp,
    pub player_start: Vec3,
    pub player_yaw: f32,
    pub goals: Vec<Goal>,
    pub lights: Vec<LightSpec>,
    pub enemies: Vec<EnemySpawn>,
}

impl Arena {
    /// Compiles the brushes and places every entity. The first `PlayerStart`
    /// wins; at least one goal is required. Never panics on hostile levels.
    pub fn from_level(level: &Level) -> Result<Self, LoadError> {
        let bsp = compile(&level.brushes).map_err(LoadError::Csg)?;
        let mut start = None;
        let (mut goals, mut lights, mut enemies) = (Vec::new(), Vec::new(), Vec::new());
        for (i, e) in level.entities.iter().enumerate() {
            if !e.is_valid() {
                return Err(LoadError::InvalidEntity(i));
            }
            match *e {
                Entity::PlayerStart { pos, yaw } => {
                    check_open(&bsp, pos, i)?;
                    start.get_or_insert((pos, yaw));
                }
                Entity::Light {
                    pos,
                    color,
                    radius,
                    intensity,
                } => lights.push(LightSpec {
                    pos,
                    color,
                    radius,
                    intensity,
                }),
                Entity::Enemy { pos, kind } => {
                    check_open(&bsp, pos, i)?;
                    enemies.push(EnemySpawn { pos, kind });
                }
                Entity::Goal { pos, radius } => goals.push(Goal { pos, radius }),
            }
        }
        let (player_start, player_yaw) = start.ok_or(LoadError::NoPlayerStart)?;
        if goals.is_empty() {
            return Err(LoadError::NoGoal);
        }
        Ok(Self {
            bsp,
            player_start,
            player_yaw,
            goals,
            lights,
            enemies,
        })
    }
}

fn check_open(bsp: &Bsp, pos: Vec3, index: usize) -> Result<(), LoadError> {
    if bsp.is_solid(pos.to_f64()) {
        Err(LoadError::SpawnInSolid(index))
    } else {
        Ok(())
    }
}
