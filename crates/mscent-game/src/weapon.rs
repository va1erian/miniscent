//! Player primary fire (M6.5): an instant ray with a cooldown.

use crate::combat::{Sphere, hitscan};
use crate::enemy::Enemy;
use crate::prop::Prop;
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
    /// Unit normal of what was struck; zero when nothing was.
    pub normal: Vec3,
    /// Mover struck, if any.
    pub mover: Option<usize>,
    /// Prop struck, if any.
    pub prop: Option<usize>,
    /// That prop was destroyed by this shot.
    pub prop_destroyed: bool,
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
        props: &mut [Prop],
    ) -> Option<Shot> {
        if !self.ready() {
            return None;
        }
        self.cooldown = FIRE_COOLDOWN_STEPS;
        let live: Vec<(usize, Sphere)> = enemies
            .iter()
            .enumerate()
            .filter(|(_, e)| e.alive())
            .flat_map(|(i, e)| e.shot_spheres().map(move |s| (i, s)))
            .collect();
        let shootable = props.iter().enumerate().filter(|(_, p)| p.shootable());
        let shootable: Vec<(usize, Sphere)> = shootable.map(|(i, p)| (i, p.sphere())).collect();
        let spheres: Vec<Sphere> = live.iter().chain(&shootable).map(|&(_, s)| s).collect();
        let result = hitscan(solids, origin, dir, WEAPON_RANGE, &spheres);
        // Targets are the enemies, then the props.
        let hit = result.target.and_then(|t| live.get(t)).map(|e| e.0);
        let prop = result
            .target
            .and_then(|t| t.checked_sub(live.len()))
            .and_then(|t| shootable.get(t))
            .map(|p| p.0);
        let killed = hit.is_some_and(|i| enemies[i].damage(WEAPON_DAMAGE));
        let prop_destroyed = prop.is_some_and(|i| props[i].damage(WEAPON_DAMAGE));
        Some(Shot {
            hit,
            killed,
            point: result.point,
            normal: result.normal,
            mover: result.mover,
            prop,
            prop_destroyed,
        })
    }
}
