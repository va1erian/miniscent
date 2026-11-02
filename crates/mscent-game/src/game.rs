//! Level flow (M6.4) and the per-step game update.
//!
//! `Game::step` is the whole simulation tick: player, weapon, enemies, movers,
//! the level script, then the win and lose checks. It reads no clock and no global state, so a
//! recorded input trace replays to the same result.

use crate::arena::Arena;
use crate::combat::{Sphere, line_of_sight};
use crate::enemy::{Enemy, Faction};
use crate::input::Input;
use crate::mover::{Mover, spawn_movers};
use crate::player::{PLAYER_RADIUS, Player};
use crate::prop::{Prop, UseState};
use crate::script::{ScriptEnd, ScriptState};
use crate::weapon::Weapon;
use mscent_collide::{FixedStepper, MoverSolid, STEP_HZ, Solids};
use mscent_math::Vec3;
use mscent_script::Vm;

/// Steps the hurt flash stays on after the player is hit.
pub const HURT_FLASH_STEPS: u32 = (STEP_HZ / 4) as u32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Playing,
    Won,
    Lost,
}

/// Things that happened, for sound, HUD and tests.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Event {
    PlayerFired,
    EnemyHit(usize),
    EnemyKilled(usize),
    /// Enemy `i` fired at the player (whether or not the shot connected).
    EnemyFired(usize),
    PlayerHurt(i32),
    PlayerDied,
    GoalReached,
    Restarted,
    /// Mover `i` was sent to a pose.
    MoverSent(usize),
    /// Mover `i` reached the pose it was sent to.
    MoverArrived(usize),
    /// Mover `i` met a body with nowhere to go. Fires once per stoppage.
    MoverBlocked(usize),
    /// The player's shot struck mover `i`.
    MoverHit(usize),
    /// The player went into zone `i`. Only a level with a script tracks zones.
    ZoneEntered(usize),
    /// The player came out of zone `i`.
    ZoneLeft(usize),
    ScriptWon,
    ScriptLost,
    /// A script spawned enemy `i`.
    EnemySpawned(usize),
    /// A script placed prop `i`.
    PropSpawned(usize),
    /// The player's shot struck prop `i`.
    PropHit(usize),
    PropDestroyed(usize),
    PropPicked(usize),
    /// The player finished holding the use key on prop `i`.
    PropUsed(usize),
    /// Where a shot went, from a player or an enemy, for effects. `normal` is
    /// zero when `struck` is [`Struck::Nothing`].
    Shot {
        shooter: Shooter,
        origin: Vec3,
        end: Vec3,
        normal: Vec3,
        struck: Struck,
    },
}

/// Who fired a shot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shooter {
    Player,
    Enemy(usize),
}

/// What a shot ended on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Struck {
    /// Out of range, or fired from inside solid.
    Nothing,
    Wall,
    Enemy(usize),
    Player,
    /// A mover, which is not in the world's BSP and has no face for a scorch to
    /// stay on where it now stands.
    Mover(usize),
    Prop(usize),
}

#[derive(Clone, Debug)]
pub struct Game {
    pub arena: Arena,
    pub player: Player,
    pub enemies: Vec<Enemy>,
    pub weapon: Weapon,
    pub phase: Phase,
    /// Fixed steps simulated since the last (re)start.
    pub ticks: u64,
    /// Steps of hurt flash remaining, for the HUD.
    pub hurt_steps: u32,
    pub movers: Vec<Mover>,
    /// Parallel to `movers`: what they collide as, at their current poses.
    pub solids: Vec<MoverSolid>,
    pub props: Vec<Prop>,
    /// The usable prop in reach, if any.
    pub(crate) use_target: Option<usize>,
    pub(crate) use_state: UseState,
    /// How many enemies are on the player's side, so a level with none pays
    /// nothing for working out who fights whom.
    pub(crate) friendlies: usize,
    pub(crate) events: Vec<Event>,
    /// The loaded script, not yet started; a restart runs a copy of it.
    pub(crate) script_program: Option<Box<Vm<Game>>>,
    pub(crate) vm: Option<Box<Vm<Game>>>,
    pub(crate) script: ScriptState,
    /// The script has the test functions, and keeps running after the level ends.
    pub(crate) script_test_mode: bool,
    stepper: FixedStepper,
}

impl Game {
    pub fn new(arena: Arena) -> Self {
        let (player, enemies) = spawn(&arena);
        let (movers, solids) = spawn_movers(&arena);
        let script = ScriptState::new(arena.zones.len(), enemies.len());
        Self {
            arena,
            player,
            enemies,
            weapon: Weapon::default(),
            phase: Phase::Playing,
            ticks: 0,
            hurt_steps: 0,
            movers,
            solids,
            props: Vec::new(),
            use_target: None,
            use_state: UseState::default(),
            friendlies: 0,
            events: Vec::new(),
            script_program: None,
            vm: None,
            script,
            script_test_mode: false,
            stepper: FixedStepper::new(),
        }
    }

    /// Puts everything back at its spawn; valid in any phase.
    pub fn restart(&mut self) {
        let (player, enemies) = spawn(&self.arena);
        self.player = player;
        self.enemies = enemies;
        (self.movers, self.solids) = spawn_movers(&self.arena);
        self.props.clear();
        self.use_target = None;
        self.use_state = UseState::default();
        self.friendlies = 0;
        self.weapon = Weapon::default();
        self.phase = Phase::Playing;
        self.ticks = 0;
        self.hurt_steps = 0;
        // Banked frame time belongs to the old run; keeping it would make a
        // restarted run step differently from a fresh one.
        self.stepper = FixedStepper::new();
        self.events.clear();
        self.events.push(Event::Restarted);
        self.start_script();
    }

    /// Returns and clears the events recorded since the last call.
    pub fn take_events(&mut self) -> Vec<Event> {
        std::mem::take(&mut self.events)
    }

    /// Feeds a frame of wall-clock time; runs the whole steps due and
    /// returns how many ran. `input` is held for each of them.
    pub fn update(&mut self, elapsed_ns: u64, input: &Input) -> u32 {
        let steps = self.stepper.advance(elapsed_ns);
        for _ in 0..steps {
            self.step(input);
        }
        steps
    }

    /// Replays a recorded trace, one input per step.
    pub fn run_trace(&mut self, trace: &[Input]) {
        for input in trace {
            self.step(input);
        }
    }

    /// One fixed step. Does nothing once the level is won or lost.
    pub fn step(&mut self, input: &Input) {
        if self.phase != Phase::Playing {
            // A test can still look at how the level ended.
            if self.script_test_mode {
                self.run_script(self.events.len(), input);
            }
            return;
        }
        let mark = self.events.len();
        self.ticks = self.ticks.saturating_add(1);
        self.hurt_steps = self.hurt_steps.saturating_sub(1);
        let solids = Solids::new(&self.arena.bsp, &self.solids);
        self.player.step(solids, input);
        self.fire(input);
        self.run_enemies();
        self.run_movers();
        self.run_script(mark, input);
        self.check_end();
    }

    fn fire(&mut self, input: &Input) {
        self.weapon.tick();
        if !input.fire {
            return;
        }
        let (origin, dir) = (self.player.pos(), self.player.forward());
        let solids = Solids::new(&self.arena.bsp, &self.solids);
        let enemies = &mut self.enemies;
        let shot = self
            .weapon
            .fire(solids, origin, dir, enemies, &mut self.props);
        let Some(shot) = shot else { return };
        self.events.push(Event::PlayerFired);
        self.events.push(Event::Shot {
            shooter: Shooter::Player,
            origin,
            end: shot.point,
            normal: shot.normal,
            struck: match (shot.hit, shot.prop) {
                (Some(i), _) => Struck::Enemy(i),
                (None, Some(i)) => Struck::Prop(i),
                (None, None) => solid_struck(shot.normal, shot.mover),
            },
        });
        if let Some(i) = shot.mover {
            self.events.push(Event::MoverHit(i));
        }
        if let Some(i) = shot.prop {
            self.events.push(Event::PropHit(i));
            if shot.prop_destroyed {
                self.events.push(Event::PropDestroyed(i));
            }
        }
        if let Some(i) = shot.hit {
            self.events.push(Event::EnemyHit(i));
            if shot.killed {
                self.events.push(Event::EnemyKilled(i));
            }
        }
    }

    fn run_enemies(&mut self) {
        if self.friendlies > 0 {
            return self.run_enemy_factions();
        }
        let target = crate::combat::Sphere {
            centre: self.player.pos(),
            radius: PLAYER_RADIUS,
        };
        let solids = Solids::new(&self.arena.bsp, &self.solids);
        for (i, e) in self.enemies.iter_mut().enumerate() {
            let dmg = e.step(solids, &target);
            report_enemy_shot(&mut self.events, i, e, Struck::Player);
            if let Some(dmg) = dmg {
                self.player.damage(dmg);
                self.hurt_steps = HURT_FLASH_STEPS;
                self.events.push(Event::PlayerHurt(dmg));
            }
        }
    }

    /// Each enemy goes after the nearest opponent within its sight. That is a
    /// search per enemy per step, paid only once a script has made one friendly.
    fn run_enemy_factions(&mut self) {
        for i in 0..self.enemies.len() {
            let target = self.nearest_opponent(i);
            let solids = Solids::new(&self.arena.bsp, &self.solids);
            let sphere = target.map(|t| self.target_sphere(t));
            let Some(enemy) = self.enemies.get_mut(i) else {
                continue;
            };
            let damage = enemy.step_toward(solids, sphere.as_ref());
            let aimed = target.flatten().map_or(Struck::Player, Struck::Enemy);
            report_enemy_shot(&mut self.events, i, enemy, aimed);
            if let (Some(damage), Some(target)) = (damage, target) {
                self.hurt_target(target, damage);
            }
        }
    }

    /// `None` is the player; `Some(j)` is enemy `j`. The outer `None` is no one.
    fn nearest_opponent(&self, i: usize) -> Option<Option<usize>> {
        let me = self.enemies.get(i)?;
        let hostile = me.faction == Faction::Hostile;
        let range = me.sight_range();
        let pos = me.pos();
        let mut best = None;
        let mut best_d = range * range;
        let player_in_range = hostile && (self.player.pos() - pos).length_squared() <= best_d;
        if player_in_range {
            best = Some(None);
            best_d = (self.player.pos() - pos).length_squared();
        }
        for (j, other) in self.enemies.iter().enumerate() {
            if j != i && other.alive() && other.faction != me.faction {
                let d = (other.pos() - pos).length_squared();
                if d < best_d {
                    best = Some(Some(j));
                    best_d = d;
                }
            }
        }
        if let Some(Some(j)) = best
            && player_in_range
            && self.player_shielded_by(pos, j)
        {
            return Some(None);
        }
        best
    }

    /// Whether enemy `j`, nearer than the player, is out of sight of `from` while
    /// the player is not, so a friendly behind a wall does not hide the player.
    fn player_shielded_by(&self, from: mscent_math::Vec3, j: usize) -> bool {
        let solids = Solids::new(&self.arena.bsp, &self.solids);
        let unseen = self
            .enemies
            .get(j)
            .is_some_and(|e| !line_of_sight(solids, from, e.pos()));
        unseen && line_of_sight(solids, from, self.player.pos())
    }

    fn target_sphere(&self, target: Option<usize>) -> Sphere {
        match target.and_then(|j| self.enemies.get(j)) {
            Some(enemy) => enemy.sphere(),
            None => Sphere {
                centre: self.player.pos(),
                radius: PLAYER_RADIUS,
            },
        }
    }

    fn hurt_target(&mut self, target: Option<usize>, damage: i32) {
        match target {
            None => {
                self.player.damage(damage);
                self.hurt_steps = HURT_FLASH_STEPS;
                self.events.push(Event::PlayerHurt(damage));
            }
            Some(j) => {
                if let Some(enemy) = self.enemies.get_mut(j) {
                    self.events.push(Event::EnemyHit(j));
                    if enemy.damage(damage) {
                        self.events.push(Event::EnemyKilled(j));
                    }
                }
            }
        }
    }

    pub(crate) fn set_enemy_faction(&mut self, i: usize, faction: Faction) {
        if let Some(enemy) = self.enemies.get_mut(i) {
            enemy.faction = faction;
        }
        let friendly = self
            .enemies
            .iter()
            .filter(|e| e.faction == Faction::Friendly);
        self.friendlies = friendly.count();
    }

    /// Death outranks the goal: dying on the same step as arriving loses.
    fn check_end(&mut self) {
        if self.player.is_dead() {
            self.phase = Phase::Lost;
            self.events.push(Event::PlayerDied);
        } else if let Some(end) = self.script.end {
            let (phase, event) = match end {
                ScriptEnd::Win => (Phase::Won, Event::ScriptWon),
                ScriptEnd::Lose => (Phase::Lost, Event::ScriptLost),
            };
            self.phase = phase;
            self.events.push(event);
        } else if self.at_goal() {
            self.phase = Phase::Won;
            self.events.push(Event::GoalReached);
        }
    }

    fn at_goal(&self) -> bool {
        let p = self.player.pos();
        self.arena.goals.iter().any(|g| {
            let reach = g.radius + PLAYER_RADIUS;
            (p - g.pos).length_squared() <= reach * reach
        })
    }

    /// Distance to the nearest goal, for the HUD.
    pub fn goal_distance(&self) -> f32 {
        let p = self.player.pos();
        self.arena
            .goals
            .iter()
            .map(|g| (p - g.pos).length())
            .fold(f32::MAX, f32::min)
    }

    /// The enemies still against the player, as the HUD counts them.
    pub fn enemies_alive(&self) -> usize {
        let against = |e: &&Enemy| e.alive() && e.faction == Faction::Hostile;
        self.enemies.iter().filter(against).count()
    }
}

/// A shot that struck no target: only something solid gives it a normal.
fn solid_struck(normal: Vec3, mover: Option<usize>) -> Struck {
    match mover {
        Some(i) => Struck::Mover(i),
        None if normal == Vec3::ZERO => Struck::Nothing,
        None => Struck::Wall,
    }
}

/// Reports the shot of enemy `i`, if it fired this step; `aimed` is what it
/// strikes if it reaches its target.
fn report_enemy_shot(events: &mut Vec<Event>, i: usize, enemy: &Enemy, aimed: Struck) {
    let Some(shot) = enemy.last_shot else { return };
    events.push(Event::EnemyFired(i));
    events.push(Event::Shot {
        shooter: Shooter::Enemy(i),
        origin: enemy.pos(),
        end: shot.point,
        normal: shot.normal,
        struck: match shot.target {
            Some(_) => aimed,
            None => solid_struck(shot.normal, shot.mover),
        },
    });
}

fn spawn(arena: &Arena) -> (Player, Vec<Enemy>) {
    let player = Player::new(arena.player_start, arena.player_yaw);
    let enemies = arena
        .enemies
        .iter()
        .enumerate()
        .map(|(i, s)| Enemy::new(i, s.pos, s.kind))
        .collect();
    (player, enemies)
}
