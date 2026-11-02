//! Fixed 120 Hz physics step with render interpolation (M4.5).
//!
//! Wall-clock time is accumulated in integer nanoseconds (scaled so one step
//! is exactly `1e9` units) so the number of steps never depends on float
//! rounding. The simulation itself only sees the constant `STEP_DT`, so
//! replaying the same inputs reproduces the same positions bit for bit.

use crate::slide::slide_move;
use crate::sweep::Solids;
use mscent_math::Vec3;

/// Physics rate.
pub const STEP_HZ: u64 = 120;
/// Duration of one physics step in seconds.
pub const STEP_DT: f32 = 1.0 / 120.0;
/// Steps run per frame at most; a longer stall drops the excess time instead
/// of spiralling.
pub const MAX_STEPS_PER_FRAME: u32 = 16;

/// Accumulator units per step: elapsed nanoseconds times `STEP_HZ`.
const UNITS_PER_STEP: u64 = 1_000_000_000;

/// Counts whole steps due from elapsed wall-clock time.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FixedStepper {
    acc: u64,
}

impl FixedStepper {
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds `elapsed_ns` and returns how many steps to run now.
    pub fn advance(&mut self, elapsed_ns: u64) -> u32 {
        // Saturating keeps absurd inputs (a debugger pause) from overflowing.
        self.acc = self.acc.saturating_add(elapsed_ns.saturating_mul(STEP_HZ));
        let due = self.acc / UNITS_PER_STEP;
        self.acc %= UNITS_PER_STEP;
        u32::try_from(due).map_or(MAX_STEPS_PER_FRAME, |d| d.min(MAX_STEPS_PER_FRAME))
    }

    /// Fraction of a step left in the accumulator, `0.0..1.0`, for rendering.
    pub fn alpha(&self) -> f32 {
        (self.acc as f64 / UNITS_PER_STEP as f64) as f32
    }
}

/// Linear interpolation between the last two simulated positions.
pub fn interpolate(prev: Vec3, cur: Vec3, alpha: f32) -> Vec3 {
    prev.lerp(cur, alpha.clamp(0.0, 1.0))
}

/// A spherical mover simulated at the fixed rate.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Body {
    pub pos: Vec3,
    /// Position one step ago, kept for interpolation.
    pub prev_pos: Vec3,
    pub vel: Vec3,
    pub radius: f32,
}

impl Body {
    pub fn new(pos: Vec3, radius: f32) -> Self {
        Self {
            pos,
            prev_pos: pos,
            vel: Vec3::ZERO,
            radius,
        }
    }

    /// One physics step under constant acceleration `accel`.
    pub fn step<'a>(&mut self, solids: impl Into<Solids<'a>>, accel: Vec3) {
        self.prev_pos = self.pos;
        let vel = self.vel + accel * STEP_DT;
        let r = slide_move(solids, self.pos, vel, STEP_DT, self.radius);
        self.pos = r.pos;
        self.vel = r.vel;
    }
}

/// A body plus its clock: feed it frame times, read back a smooth position.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Simulation {
    pub body: Body,
    stepper: FixedStepper,
}

impl Simulation {
    pub fn new(pos: Vec3, radius: f32) -> Self {
        Self {
            body: Body::new(pos, radius),
            stepper: FixedStepper::new(),
        }
    }

    /// Advances by a frame of `elapsed_ns`, applying `accel` on every step;
    /// returns the number of steps run.
    pub fn update<'a>(
        &mut self,
        solids: impl Into<Solids<'a>>,
        elapsed_ns: u64,
        accel: Vec3,
    ) -> u32 {
        let solids = solids.into();
        let steps = self.stepper.advance(elapsed_ns);
        for _ in 0..steps {
            self.body.step(solids, accel);
        }
        steps
    }

    /// Position to draw this frame, between the last two steps.
    pub fn render_pos(&self) -> Vec3 {
        interpolate(self.body.prev_pos, self.body.pos, self.stepper.alpha())
    }
}
