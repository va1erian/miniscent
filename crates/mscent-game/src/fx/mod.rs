//! Particle simulation (M9.2): class pools, emitters, a 60 Hz tick and three
//! collision levels. Drawing lives elsewhere; this module only moves points.
//!
//! # Public API
//!
//! * [`Fx::new`] allocates every pool for a `'static` class table; nothing
//!   allocates afterwards.
//! * [`Fx::spawn`] fires an [`Effect`] (a table of [`Emitter`]s) with
//!   [`EffectArgs`]; hostile arguments are rejected or clamped, never a panic.
//! * [`Fx::advance`] runs the particle clock: whole 60 Hz ticks for the time
//!   given, at most [`MAX_CATCHUP_TICKS`] per call so a stall cannot spiral.
//! * [`Fx::for_each`] reads every live particle, interpolated by
//!   [`Fx::alpha`].
//!
//! `Fx` owns its seeded `Rng` and never touches `Game`, so effects cannot
//! change gameplay or break a replay.

pub mod budget;
pub mod class;
mod collect;
pub mod collide;
pub mod combat;
pub mod decal;
pub mod emit;
mod pool;
pub mod recipes;

pub use class::{
    Blend, ClassId, Collide, MAX_CLASSES, MAX_TOTAL_CAPACITY, ParticleClass, Prim, PuffShape, Tier,
    Trail,
};
pub use collide::{Budget, MAX_SOLID_TESTS, MAX_SWEEPS};
pub use emit::{Effect, EffectArgs, Emitter, QUEUE_CAPACITY, Shape, TintSource};

use mscent_math::Vec3;
use mscent_math::rng::Rng;
use mscent_world::Bsp;
use pool::{Born, Pool};

/// Particle ticks per second.
pub const TICK_HZ: u64 = 60;
/// Seconds per tick.
pub const TICK_DT: f32 = 1.0 / 60.0;
/// Ticks `advance` runs per call at most. Four is a 67 ms stall: longer
/// ones (a window drag, a debugger) drop the excess time, because replaying
/// seconds of smoke after a pause would cost far more than it is worth.
pub const MAX_CATCHUP_TICKS: u32 = 4;
/// Particles leaving this cube die. It is far beyond any level, and keeps
/// every position finite whatever speed an emitter asked for.
pub const WORLD_LIMIT: f32 = 1.0e6;
/// Gravity direction. The design puts it along -Z; one line to change.
const GRAVITY_AXIS: Vec3 = Vec3::new(0.0, 0.0, -1.0);
/// Seconds a shed trail particle lives.
const TRAIL_LIFE: f32 = 0.6;
/// Trail particles one class can shed in one tick; the rest are skipped.
const MAX_SHED: usize = 64;
const NS_PER_SEC: u64 = 1_000_000_000;
/// `LiveParticle::key` takes every byte value.
const ALL_KEYS: u16 = 256;

/// One live particle as the renderer sees it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LiveParticle {
    pub class: ClassId,
    /// Already includes `vel * alpha * dt`.
    pub pos: Vec3,
    pub vel: Vec3,
    /// Age as a fraction of life, `0..1`.
    pub t: f32,
    /// Fixed at birth from the seeded generator; the budget thins by it.
    pub key: u8,
    /// Mean albedo `0xRRGGBB`; 0 means "use the class ramp".
    pub surface: u32,
}

/// Accumulates wall-clock nanoseconds into whole ticks. Scaled by the tick
/// rate so the tick count never depends on float rounding. (The collide
/// crate's `FixedStepper` is fixed at 120 Hz.)
#[derive(Clone, Copy, Debug, Default)]
struct Clock {
    acc: u64,
}

impl Clock {
    fn advance(&mut self, elapsed_ns: u64) -> u32 {
        self.acc = self.acc.saturating_add(elapsed_ns.saturating_mul(TICK_HZ));
        let due = self.acc / NS_PER_SEC;
        self.acc %= NS_PER_SEC;
        u32::try_from(due).map_or(MAX_CATCHUP_TICKS, |d| d.min(MAX_CATCHUP_TICKS))
    }

    fn alpha(&self) -> f32 {
        (self.acc as f64 / NS_PER_SEC as f64) as f32
    }
}

/// What a class does each tick, with its table values made safe.
struct Motion {
    gravity: f32,
    drag_keep: f32,
    mode: Collide,
}

pub struct Fx {
    classes: &'static [ParticleClass],
    pools: Vec<Pool>,
    /// Class indices with Bounce first, then Die, then None, so the tick's
    /// collision limits are spent where they matter most.
    order: Vec<u8>,
    rng: Rng,
    clock: Clock,
    queue: emit::Queue,
    budget: Budget,
    /// Normal and Cosmetic particles with a key at or above this are not born.
    density_keys: u16,
}

impl Fx {
    /// Allocates a pool per class (at most [`MAX_CLASSES`] classes and
    /// [`MAX_TOTAL_CAPACITY`] particles in all). This is the only allocation.
    pub fn new(classes: &'static [ParticleClass], seed: u64) -> Self {
        let classes = &classes[..classes.len().min(MAX_CLASSES)];
        let mut room = MAX_TOTAL_CAPACITY;
        let pools = classes
            .iter()
            .map(|c| {
                let cap = usize::from(c.capacity).min(room);
                room -= cap;
                Pool::new(cap)
            })
            .collect();
        Self {
            classes,
            pools,
            order: collision_order(classes),
            rng: Rng::new(seed),
            clock: Clock::default(),
            queue: emit::Queue::default(),
            budget: Budget::default(),
            density_keys: ALL_KEYS,
        }
    }

    /// The fraction of Normal and Cosmetic particles spawned from now on.
    pub fn set_density(&mut self, density: f32) {
        // A NaN casts to 0, which spawns only what is Essential.
        self.density_keys = (density.clamp(0.0, 1.0) * f32::from(ALL_KEYS)) as u16;
    }

    /// Fires every emitter of `effect`; delayed ones wait in the queue.
    /// Non-finite arguments reject the whole effect. Scale and counts are
    /// clamped, a full pool drops (Essential replaces its oldest), and a full
    /// queue drops the emitter.
    pub fn spawn(&mut self, effect: Effect, args: &EffectArgs) {
        let Some(args) = emit::validated(args) else {
            return;
        };
        for e in effect {
            let delay = emit::delay_of(e);
            if delay > 0.0 {
                self.queue.push(e, args, delay);
            } else {
                self.emit(e, &args);
            }
        }
    }

    /// Runs the particle clock forward by `elapsed_ns`.
    pub fn advance(&mut self, elapsed_ns: u64, bsp: &Bsp) {
        for _ in 0..self.clock.advance(elapsed_ns) {
            self.tick(bsp);
        }
    }

    /// Fraction of a tick elapsed since the last one, `0..1`.
    pub fn alpha(&self) -> f32 {
        self.clock.alpha()
    }

    /// Removes every particle and waiting emitter and restarts the clock.
    pub fn clear(&mut self) {
        self.pools.iter_mut().for_each(Pool::clear);
        self.queue.clear();
        self.clock = Clock::default();
    }

    pub fn live(&self) -> usize {
        self.pools.iter().map(Pool::len).sum()
    }

    /// Emitters still waiting out their delay.
    pub fn queued(&self) -> usize {
        self.queue.waiting()
    }

    /// Collision work the most recent tick spent; never above
    /// [`MAX_SOLID_TESTS`] and [`MAX_SWEEPS`].
    pub fn last_tick_collision(&self) -> Budget {
        self.budget
    }

    /// Calls `f` for every live particle, class by class. Allocation free.
    pub fn for_each(&self, mut f: impl FnMut(LiveParticle)) {
        let lead = self.alpha() * TICK_DT;
        for (ci, pool) in self.pools.iter().enumerate() {
            for i in 0..pool.len() {
                let vel = pool.vel(i);
                f(LiveParticle {
                    // `new` keeps at most 256 classes, so the index fits.
                    class: ClassId(ci as u8),
                    pos: pool.pos(i) + vel * lead,
                    vel,
                    t: pool.t(i),
                    key: pool.key(i),
                    surface: pool.surface(i),
                });
            }
        }
    }

    fn pool_index(&self, id: ClassId) -> Option<usize> {
        let i = usize::from(id.0);
        (i < self.pools.len()).then_some(i)
    }

    fn emit(&mut self, e: &Emitter, args: &EffectArgs) {
        let Some(ci) = self.pool_index(e.class) else {
            return;
        };
        let replace = self.classes[ci].tier == Tier::Essential;
        for _ in 0..emit::count_of(e, &mut self.rng) {
            let born = emit::born(e, args, &mut self.rng);
            if replace || u16::from(born.key) < self.density_keys {
                self.pools[ci].push(&born, replace);
            }
        }
    }

    fn tick(&mut self, bsp: &Bsp) {
        self.budget = Budget::default();
        for slot in 0..emit::QUEUE_CAPACITY {
            if let Some(p) = self.queue.take_due(slot, TICK_DT) {
                self.emit(p.emitter, &p.args);
            }
        }
        for k in 0..self.order.len() {
            self.step_class(usize::from(self.order[k]), bsp);
        }
    }

    fn step_class(&mut self, ci: usize, bsp: &Bsp) {
        let class = self.classes[ci];
        let motion = Motion::of(&class);
        let interval = class
            .trail
            .filter(|t| t.interval.is_finite() && t.interval > 0.0);
        let pool = &mut self.pools[ci];
        // Shed positions are staged because the trail class's pool cannot
        // be borrowed while this one is.
        let mut shed = [Vec3::ZERO; MAX_SHED];
        let mut shed_n = 0;
        let mut i = 0;
        while i < pool.len() {
            if !step_particle(pool, i, &motion, bsp, &mut self.budget) {
                // The last particle now sits at `i` and has not stepped yet.
                pool.swap_remove(i);
                continue;
            }
            if let Some(t) = interval {
                let acc = pool.trail_acc(i) + TICK_DT;
                let due = acc >= t.interval;
                let acc = if due { acc - t.interval } else { acc };
                pool.set_trail_acc(i, acc.min(t.interval));
                if due && shed_n < MAX_SHED {
                    shed[shed_n] = pool.pos(i);
                    shed_n += 1;
                }
            }
            i += 1;
        }
        if let Some(t) = class.trail {
            self.shed_trail(t.class, &shed[..shed_n]);
        }
    }

    fn shed_trail(&mut self, id: ClassId, at: &[Vec3]) {
        let Some(ci) = self.pool_index(id) else {
            return;
        };
        let replace = self.classes[ci].tier == Tier::Essential;
        for &pos in at {
            let born = Born {
                pos,
                vel: Vec3::ZERO,
                life: TRAIL_LIFE,
                key: self.rng.below(256) as u8,
                surface: 0,
            };
            self.pools[ci].push(&born, replace);
        }
    }
}

impl Motion {
    fn of(class: &ParticleClass) -> Self {
        let finite = |v: f32| if v.is_finite() { v } else { 0.0 };
        Self {
            gravity: finite(class.gravity),
            drag_keep: (1.0 - finite(class.drag) * TICK_DT).clamp(0.0, 1.0),
            mode: class.collide,
        }
    }
}

/// Semi-implicit Euler: velocity first, then position from the new velocity.
/// Returns false when the particle died this tick.
fn step_particle(pool: &mut Pool, i: usize, m: &Motion, bsp: &Bsp, budget: &mut Budget) -> bool {
    let t = pool.t(i) + TICK_DT * pool.inv_life(i);
    if t >= 1.0 {
        return false;
    }
    let prev = pool.pos(i);
    let vel = (pool.vel(i) + GRAVITY_AXIS * (m.gravity * TICK_DT)) * m.drag_keep;
    let pos = prev + vel * TICK_DT;
    if !(pos.is_finite() && vel.is_finite() && emit::within_world(pos)) {
        return false;
    }
    let (pos, vel) = match collide::resolve(m.mode, bsp, budget, prev, pos, vel) {
        collide::Outcome::Free => (pos, vel),
        collide::Outcome::Dead => return false,
        collide::Outcome::Bounced { pos, vel } => (pos, vel),
    };
    pool.set_pos_vel(i, pos, vel);
    pool.set_t(i, t);
    true
}

fn collision_order(classes: &[ParticleClass]) -> Vec<u8> {
    let rank = |c: &ParticleClass| match c.collide {
        Collide::Bounce { .. } => 0,
        Collide::Die => 1,
        Collide::None => 2,
    };
    let mut order = Vec::with_capacity(classes.len());
    for r in 0..3 {
        for (i, c) in classes.iter().enumerate() {
            if rank(c) == r {
                order.push(i as u8);
            }
        }
    }
    order
}

#[cfg(test)]
#[path = "../fx_collide_tests.rs"]
mod fx_collide_tests;
#[cfg(test)]
#[path = "../fx_tests.rs"]
mod fx_tests;
