//! Level flow (M6.4) and the per-step game update.
//!
//! `Game::step` is the whole simulation tick: player, weapon, enemies, then
//! the win and lose checks. It reads no clock and no global state, so a
//! recorded input trace replays to the same result.

use crate::arena::Arena;
use crate::enemy::Enemy;
use crate::input::Input;
use crate::player::{PLAYER_RADIUS, Player};
use crate::weapon::Weapon;
use mscent_collide::{FixedStepper, STEP_HZ};

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
    PlayerHurt(i32),
    PlayerDied,
    GoalReached,
    Restarted,
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
    events: Vec<Event>,
    stepper: FixedStepper,
}

impl Game {
    pub fn new(arena: Arena) -> Self {
        let (player, enemies) = spawn(&arena);
        Self {
            arena,
            player,
            enemies,
            weapon: Weapon::default(),
            phase: Phase::Playing,
            ticks: 0,
            hurt_steps: 0,
            events: Vec::new(),
            stepper: FixedStepper::new(),
        }
    }

    /// Puts everything back at its spawn; valid in any phase.
    pub fn restart(&mut self) {
        let (player, enemies) = spawn(&self.arena);
        self.player = player;
        self.enemies = enemies;
        self.weapon = Weapon::default();
        self.phase = Phase::Playing;
        self.ticks = 0;
        self.hurt_steps = 0;
        // Banked frame time belongs to the old run; keeping it would make a
        // restarted run step differently from a fresh one.
        self.stepper = FixedStepper::new();
        self.events.clear();
        self.events.push(Event::Restarted);
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
            return;
        }
        self.ticks = self.ticks.saturating_add(1);
        self.hurt_steps = self.hurt_steps.saturating_sub(1);
        self.player.step(&self.arena.bsp, input);
        self.fire(input);
        self.run_enemies();
        self.check_end();
    }

    fn fire(&mut self, input: &Input) {
        self.weapon.tick();
        if !input.fire {
            return;
        }
        let (origin, dir) = (self.player.pos(), self.player.forward());
        let shot = self
            .weapon
            .fire(&self.arena.bsp, origin, dir, &mut self.enemies);
        let Some(shot) = shot else { return };
        self.events.push(Event::PlayerFired);
        if let Some(i) = shot.hit {
            self.events.push(Event::EnemyHit(i));
            if shot.killed {
                self.events.push(Event::EnemyKilled(i));
            }
        }
    }

    fn run_enemies(&mut self) {
        let target = crate::combat::Sphere {
            centre: self.player.pos(),
            radius: PLAYER_RADIUS,
        };
        for e in &mut self.enemies {
            if let Some(dmg) = e.step(&self.arena.bsp, &target) {
                self.player.damage(dmg);
                self.hurt_steps = HURT_FLASH_STEPS;
                self.events.push(Event::PlayerHurt(dmg));
            }
        }
    }

    /// Death outranks the goal: dying on the same step as arriving loses.
    fn check_end(&mut self) {
        if self.player.is_dead() {
            self.phase = Phase::Lost;
            self.events.push(Event::PlayerDied);
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

    pub fn enemies_alive(&self) -> usize {
        self.enemies.iter().filter(|e| e.alive()).count()
    }
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
