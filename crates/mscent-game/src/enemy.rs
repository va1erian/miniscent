//! Enemy behaviour (M6.3): patrol, chase and attack, with sight decided by a
//! ray through the BSP.
//!
//! The state machine is `Patrol -> Chase -> Attack` and back. An enemy that
//! loses sight keeps heading for where it last saw the player and gives up
//! after `lose_steps`; seeing the player again at any point re-acquires.
//! Randomness comes from a per-enemy seeded generator, so replays match.

use crate::combat::{Sphere, hitscan, line_of_sight};
use mscent_collide::{Body, STEP_HZ, Solids, sweep_sphere};
use mscent_math::Vec3;
use mscent_math::rng::Rng;

/// Which side an enemy is on. A friendly one fights the hostile ones.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Faction {
    Hostile,
    Friendly,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AiState {
    Patrol,
    Chase,
    Attack,
    Dead,
}

/// Per-kind tuning. Times are in fixed steps.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EnemyStats {
    pub max_hp: i32,
    pub radius: f32,
    pub speed: f32,
    pub sight_range: f32,
    pub attack_range: f32,
    pub attack_damage: i32,
    pub attack_cooldown: u32,
    pub lose_steps: u32,
}

/// Stats for an entity `kind`. Unknown kinds use the basic drone, so a level
/// from a newer editor still loads.
pub fn stats_for(kind: u8) -> EnemyStats {
    let drone = EnemyStats {
        max_hp: 50,
        radius: 0.75,
        speed: 5.0,
        sight_range: 40.0,
        attack_range: 9.0,
        attack_damage: 6,
        attack_cooldown: (STEP_HZ / 2) as u32,
        lose_steps: (STEP_HZ * 3) as u32,
    };
    match kind {
        1 => EnemyStats {
            max_hp: 100,
            speed: 7.0,
            attack_damage: 12,
            attack_cooldown: STEP_HZ as u32,
            ..drone
        },
        _ => drone,
    }
}

/// Distance at which a chaser counts as having reached the last-seen spot.
const ARRIVE: f32 = 1.0;
/// Patrol speed as a fraction of chase speed.
const PATROL_SPEED_FRACTION: f32 = 0.4;
/// How far ahead a patroller looks for walls.
const LOOKAHEAD: f32 = 3.0;
/// Random headings tried before a patroller just turns around.
const TURN_TRIES: usize = 8;
/// An attacker keeps firing until the player is this much farther than range.
const ATTACK_HYSTERESIS: f32 = 1.25;

#[derive(Clone, Debug)]
pub struct Enemy {
    pub body: Body,
    pub kind: u8,
    pub hp: i32,
    pub state: AiState,
    /// Where the player was last in sight, while hunting.
    pub last_seen: Option<Vec3>,
    /// True for the step in which this enemy fired (hit or miss), so the
    /// game can show where the shot came from.
    pub fired: bool,
    /// A disabled enemy stays where it is, can still be shot, and does nothing.
    pub enabled: bool,
    pub faction: Faction,
    stats: EnemyStats,
    lost_steps: u32,
    cooldown: u32,
    patrol_dir: Vec3,
    rng: Rng,
}

impl Enemy {
    /// `index` seeds the generator so each enemy wanders differently.
    pub fn new(index: usize, pos: Vec3, kind: u8) -> Self {
        Self::with_stats(index, pos, kind, stats_for(kind))
    }

    /// An enemy that looks like `kind` and plays by `stats`.
    pub fn with_stats(index: usize, pos: Vec3, kind: u8, stats: EnemyStats) -> Self {
        let mut rng = Rng::new(0x5EED_0000 ^ index as u64);
        let patrol_dir = random_dir(&mut rng);
        Self {
            body: Body::new(pos, stats.radius),
            kind,
            hp: stats.max_hp,
            state: AiState::Patrol,
            last_seen: None,
            fired: false,
            enabled: true,
            faction: Faction::Hostile,
            stats,
            lost_steps: 0,
            cooldown: stats.attack_cooldown,
            patrol_dir,
            rng,
        }
    }

    pub fn alive(&self) -> bool {
        self.state != AiState::Dead
    }

    pub fn pos(&self) -> Vec3 {
        self.body.pos
    }

    pub fn sphere(&self) -> Sphere {
        Sphere {
            centre: self.body.pos,
            radius: self.body.radius,
        }
    }

    /// Applies damage; returns true if this killed the enemy.
    pub fn damage(&mut self, amount: i32) -> bool {
        if !self.alive() {
            return false;
        }
        self.hp = self.hp.saturating_sub(amount.max(0));
        if self.hp > 0 {
            return false;
        }
        self.state = AiState::Dead;
        self.body.vel = Vec3::ZERO;
        true
    }

    /// One fixed step. Returns the damage dealt to the player this step.
    pub fn step<'a>(&mut self, solids: impl Into<Solids<'a>>, player: &Sphere) -> Option<i32> {
        self.step_toward(solids.into(), Some(player))
    }

    /// How far it can see, for choosing who to go after.
    pub fn sight_range(&self) -> f32 {
        self.stats.sight_range
    }

    /// One fixed step with `target` as what it is after, or nothing in reach.
    /// Returns the damage dealt to the target this step.
    pub(crate) fn step_toward(&mut self, bsp: Solids, target: Option<&Sphere>) -> Option<i32> {
        if !self.alive() {
            return None;
        }
        self.fired = false;
        if !self.enabled {
            // A mover can still push it, and it draws between these two.
            self.body.prev_pos = self.body.pos;
            return None;
        }
        self.cooldown = self.cooldown.saturating_sub(1);
        let Some(player) = target else {
            // Nothing to go after: lose interest in the usual way, and patrol.
            self.track(false, self.body.pos);
            self.transition(false, f32::MAX);
            match self.state {
                AiState::Patrol => self.patrol(bsp),
                AiState::Chase | AiState::Attack => self.chase(bsp),
                AiState::Dead => {}
            }
            return None;
        };
        let to_player = player.centre - self.body.pos;
        let dist = to_player.length();
        let sees =
            dist <= self.stats.sight_range && line_of_sight(bsp, self.body.pos, player.centre);
        self.track(sees, player.centre);
        self.transition(sees, dist);
        match self.state {
            AiState::Patrol => self.patrol(bsp),
            AiState::Chase => self.chase(bsp),
            AiState::Attack => return self.attack(bsp, player),
            AiState::Dead => {}
        }
        None
    }

    /// Remembers the player while visible, otherwise counts down to giving up.
    fn track(&mut self, sees: bool, player_pos: Vec3) {
        if sees {
            self.last_seen = Some(player_pos);
            self.lost_steps = 0;
        } else if self.last_seen.is_some() {
            self.lost_steps = self.lost_steps.saturating_add(1);
        }
    }

    fn transition(&mut self, sees: bool, dist: f32) {
        let range = self.stats.attack_range;
        self.state = match self.state {
            AiState::Patrol if sees => AiState::Chase,
            AiState::Chase if sees && dist <= range => AiState::Attack,
            AiState::Chase if !sees && self.lost_steps >= self.stats.lose_steps => {
                self.last_seen = None;
                self.lost_steps = 0;
                AiState::Patrol
            }
            AiState::Attack if !sees || dist > range * ATTACK_HYSTERESIS => AiState::Chase,
            s => s,
        };
    }

    fn patrol(&mut self, bsp: Solids) {
        let r = self.body.radius;
        let ahead = self.body.pos + self.patrol_dir * LOOKAHEAD;
        if sweep_sphere(bsp, self.body.pos, ahead, r).hit {
            self.patrol_dir = self.pick_clear_dir(bsp);
        }
        self.glide(
            self.patrol_dir * (self.stats.speed * PATROL_SPEED_FRACTION),
            bsp,
        );
    }

    /// A random heading with room ahead, or straight back if none is found.
    fn pick_clear_dir(&mut self, bsp: Solids) -> Vec3 {
        for _ in 0..TURN_TRIES {
            let d = random_dir(&mut self.rng);
            let end = self.body.pos + d * LOOKAHEAD;
            if !sweep_sphere(bsp, self.body.pos, end, self.body.radius).hit {
                return d;
            }
        }
        -self.patrol_dir
    }

    fn chase(&mut self, bsp: Solids) {
        let Some(goal) = self.last_seen else { return };
        let to = goal - self.body.pos;
        let vel = match to.normalized() {
            Some(d) if to.length() > ARRIVE => d * self.stats.speed,
            _ => Vec3::ZERO,
        };
        self.glide(vel, bsp);
    }

    /// Stands and shoots; the shot is a ray, so cover works both ways.
    fn attack(&mut self, bsp: Solids, player: &Sphere) -> Option<i32> {
        self.glide(Vec3::ZERO, bsp);
        if self.cooldown > 0 {
            return None;
        }
        self.cooldown = self.stats.attack_cooldown;
        self.fired = true;
        let reach = self.stats.attack_range * ATTACK_HYSTERESIS;
        let dir = player.centre - self.body.pos;
        let shot = hitscan(bsp, self.body.pos, dir, reach, &[*player]);
        shot.target.map(|_| self.stats.attack_damage)
    }

    /// Moves with the collision slide at the given velocity.
    fn glide(&mut self, vel: Vec3, bsp: Solids) {
        self.body.vel = vel;
        self.body.step(bsp, Vec3::ZERO);
    }
}

/// A uniformly spread unit vector, by rejection from the unit cube.
fn random_dir(rng: &mut Rng) -> Vec3 {
    for _ in 0..16 {
        let v = Vec3::new(
            rng.range_f32(-1.0, 1.0),
            rng.range_f32(-1.0, 1.0),
            rng.range_f32(-1.0, 1.0),
        );
        if v.length_squared() <= 1.0
            && let Some(d) = v.normalized()
        {
            return d;
        }
    }
    Vec3::new(1.0, 0.0, 0.0)
}
