//! Player primary fire (M6.5): an instant ray with a cooldown.

use crate::combat::{Sphere, hitscan};
use crate::enemy::Enemy;
use mscent_collide::{STEP_HZ, Solids};
use mscent_math::Vec3;

/// Shots reach this far.
pub const WEAPON_RANGE: f32 = 200.0;
/// Health taken from an enemy per hit.
pub const WEAPON_DAMAGE: i32 = 25;
/// Steps between shots (a quarter second).
pub const FIRE_COOLDOWN_STEPS: u32 = (STEP_HZ / 4) as u32;

/// What a shot did.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shot {
    /// Enemy index struck, if any.
    pub hit: Option<usize>,
    pub killed: bool,
    pub point: Vec3,
    /// Mover struck, if any.
    pub mover: Option<usize>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Weapon {
    cooldown: u32,
}

impl Weapon {
    pub fn ready(&self) -> bool {
        self.cooldown == 0
    }

    pub fn tick(&mut self) {
        self.cooldown = self.cooldown.saturating_sub(1);
    }

    /// Fires if ready. Dead enemies are not targets.
    pub fn fire<'a>(
        &mut self,
        solids: impl Into<Solids<'a>>,
        origin: Vec3,
        dir: Vec3,
        enemies: &mut [Enemy],
    ) -> Option<Shot> {
        if !self.ready() {
            return None;
        }
        self.cooldown = FIRE_COOLDOWN_STEPS;
        let live: Vec<(usize, Sphere)> = enemies
            .iter()
            .enumerate()
            .filter(|(_, e)| e.alive())
            .map(|(i, e)| (i, e.sphere()))
            .collect();
        let spheres: Vec<Sphere> = live.iter().map(|&(_, s)| s).collect();
        let result = hitscan(solids, origin, dir, WEAPON_RANGE, &spheres);
        let hit = result.target.map(|t| live[t].0);
        let killed = hit.is_some_and(|i| enemies[i].damage(WEAPON_DAMAGE));
        Some(Shot {
            hit,
            killed,
            point: result.point,
            mover: result.mover,
        })
    }
}
