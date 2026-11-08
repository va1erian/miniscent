//! A level's enemy kinds as the simulation plays them (M19.2): stats in whole
//! steps, and the spheres a shot can strike.
//!
//! The simulation reads a kind's stats and its blueprint's `hit` lines and
//! nothing else of it, so a design's mesh can change without changing play. The
//! stock table, which a level with no kinds of its own plays, has no hit spheres:
//! shots strike the body, as they did before kinds, so old replays match.

use std::sync::Arc;

use mscent_collide::STEP_HZ;
use mscent_math::Vec3;
use mscent_world::{KindStats, Level};

use crate::combat::Sphere;
use crate::enemy::EnemyStats;

/// One kind of enemy in a level, as played.
#[derive(Clone, Debug, PartialEq)]
pub struct KindPlay {
    pub name: String,
    pub stats: EnemyStats,
    /// Spheres a shot strikes, about the body's centre with the enemy facing
    /// +X, in world units. Empty means the body itself.
    pub hits: Arc<[Sphere]>,
}

/// The kinds `level` plays, in its order, which `Entity::Enemy::kind` indexes.
pub fn kinds_of(level: &Level) -> Vec<KindPlay> {
    let own = !level.kinds.is_empty();
    let play = |k: &mscent_world::Kind| {
        let hits: Arc<[Sphere]> = if own {
            hits_of(&k.blueprint).into()
        } else {
            Arc::new([])
        };
        let mut stats = stats_of(&k.stats);
        // The body that meets the world holds what a shot can strike, so no
        // wing passes through a wall the body stops at.
        stats.radius = hits.iter().map(reach).fold(stats.radius, f32::max);
        KindPlay {
            name: k.name.clone(),
            stats,
            hits,
        }
    };
    level.kind_table().iter().map(play).collect()
}

/// `s` with its times turned into whole fixed steps.
pub fn stats_of(s: &KindStats) -> EnemyStats {
    EnemyStats {
        max_hp: i32::try_from(s.hp).unwrap_or(i32::MAX),
        radius: s.radius,
        speed: s.speed,
        sight_range: s.sight,
        attack_range: s.attack_range,
        attack_damage: i32::try_from(s.damage).unwrap_or(i32::MAX),
        attack_cooldown: steps(s.cooldown),
        lose_steps: steps(s.lose),
    }
}

/// Seconds as whole steps. `KindStats::is_valid` bounds them, so this fits.
fn steps(seconds: f32) -> u32 {
    let n = (seconds * STEP_HZ as f32).round();
    if n.is_finite() && n >= 0.0 {
        n.min(u32::MAX as f32) as u32
    } else {
        0
    }
}

/// The blueprint's hit spheres in world units, about its origin.
fn hits_of(b: &mscent_world::model::Blueprint) -> Vec<Sphere> {
    let s = b.scale;
    let sphere = |h: &mscent_world::model::Hit| Sphere {
        centre: Vec3::new(
            (h.centre.x * s) as f32,
            (h.centre.y * s) as f32,
            (h.centre.z * s) as f32,
        ),
        radius: (h.radius * s) as f32,
    };
    b.hits.iter().map(sphere).collect()
}

/// How far from the body's centre a sphere reaches.
fn reach(s: &Sphere) -> f32 {
    s.centre.length() + s.radius
}
