//! The 6DoF player (M6.1): thrust on three axes, pitch, yaw and roll held as
//! a quaternion, and inertia on both motion and rotation.
//!
//! Orientation is a unit quaternion updated by body-frame increments, so no
//! pitch angle is singular: looping through 90 degrees up behaves like any
//! other angle. Motion is the `mscent_collide` fixed step; `Player::step`
//! is called once per `STEP_DT` and nothing here reads a clock.

use crate::input::Input;
use mscent_collide::{Body, STEP_DT};
use mscent_math::{Quat, Vec3};
use mscent_world::Bsp;

/// Collision sphere radius.
pub const PLAYER_RADIUS: f32 = 0.5;
/// Thrust acceleration at full stick, units per second squared.
pub const THRUST_ACCEL: f32 = 36.0;
/// Linear drag per second; terminal speed is `THRUST_ACCEL / DRAG` (12 m/s,
/// slow enough to thread a 4 m doorway).
pub const DRAG: f32 = 3.0;
/// Extra drag per second while no thrust is held, so the ship stops quickly.
pub const BRAKE: f32 = 5.0;
/// Extra drag per second on the velocity sideways to the thrust, so the
/// ship follows where it is pointed instead of drifting.
pub const STEER: f32 = 6.0;
/// How fast a free roll returns to level, per second (proportional).
pub const LEVEL_RATE: f32 = 2.5;
/// Turn rate at full stick, radians per second.
pub const MAX_TURN_RATE: f32 = 2.5;
/// How fast the turn rate follows the stick, radians per second squared.
pub const TURN_ACCEL: f32 = 12.0;
/// Starting and maximum health.
pub const MAX_HEALTH: i32 = 100;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Player {
    pub body: Body,
    pub orientation: Quat,
    /// Angular velocity in the body frame, radians per second.
    pub ang_vel: Vec3,
    pub health: i32,
    /// World-space up used by mouse yaw and roll self-levelling; `None` is
    /// pure 6DoF with no preferred up.
    pub up_ref: Option<Vec3>,
}

impl Player {
    /// Spawns at rest facing `yaw`: 0 looks along -Z, positive turns left
    /// about +Y. A non-finite yaw is treated as 0.
    pub fn new(pos: Vec3, yaw: f32) -> Self {
        let yaw = if yaw.is_finite() { yaw } else { 0.0 };
        Self {
            body: Body::new(pos, PLAYER_RADIUS),
            orientation: Quat::from_axis_angle(Vec3::new(0.0, 1.0, 0.0), yaw),
            ang_vel: Vec3::ZERO,
            health: MAX_HEALTH,
            up_ref: None,
        }
    }

    pub fn pos(&self) -> Vec3 {
        self.body.pos
    }

    pub fn forward(&self) -> Vec3 {
        self.orientation.rotate(Vec3::new(0.0, 0.0, -1.0))
    }

    pub fn right(&self) -> Vec3 {
        self.orientation.rotate(Vec3::new(1.0, 0.0, 0.0))
    }

    pub fn up(&self) -> Vec3 {
        self.orientation.rotate(Vec3::new(0.0, 1.0, 0.0))
    }

    pub fn is_dead(&self) -> bool {
        self.health <= 0
    }

    /// Subtracts health, saturating so a huge hit cannot overflow.
    pub fn damage(&mut self, amount: i32) {
        self.health = self.health.saturating_sub(amount.max(0)).max(0);
    }

    /// One fixed step of rotation then translation under `input`.
    pub fn step(&mut self, bsp: &Bsp, input: &Input) {
        let i = input.sanitized();
        self.turn(Vec3::new(i.pitch, i.yaw, i.roll));
        // Forward on the stick is -Z in the body frame.
        let local = Vec3::new(i.thrust.x, i.thrust.y, -i.thrust.z);
        let thrust = self.orientation.rotate(local) * THRUST_ACCEL;
        let vel = self.body.vel;
        let mut drag = vel * DRAG;
        if local.length() < 1e-3 {
            drag = drag + vel * BRAKE;
        } else {
            let dir = thrust * (1.0 / THRUST_ACCEL);
            let dir = dir * (1.0 / dir.length().max(1e-6));
            drag = drag + (vel - dir * vel.dot(dir)) * STEER;
        }
        self.body.step(bsp, thrust - drag);
        if let (0.0, Some(up)) = (i.roll, self.up_ref) {
            self.level(up);
        }
    }

    /// Immediate view rotation (mouse look): `pitch` about the body right
    /// axis, `yaw` about the world up, so looking up and turning never
    /// accumulates roll. Upside down, yaw is mirrored to keep its feel.
    pub fn look(&mut self, pitch: f32, yaw: f32) {
        if !(pitch.is_finite() && yaw.is_finite()) {
            return;
        }
        let pitch_q = Quat::from_axis_angle(Vec3::new(1.0, 0.0, 0.0), pitch);
        let Some(up) = self.up_ref else {
            let yaw_q = Quat::from_axis_angle(Vec3::new(0.0, 1.0, 0.0), yaw);
            self.orientation = (self.orientation * yaw_q * pitch_q).normalized();
            return;
        };
        let sign = if self.up().dot(up) < 0.0 { -1.0 } else { 1.0 };
        let yaw_q = Quat::from_axis_angle(up, yaw * sign);
        self.orientation = (yaw_q * self.orientation * pitch_q).normalized();
    }

    /// Eases the roll back toward upright about the view axis.
    fn level(&mut self, world_up: Vec3) {
        let err = -self.right().dot(world_up).atan2(self.up().dot(world_up));
        let k = (LEVEL_RATE * STEP_DT).min(1.0);
        let delta = Quat::from_axis_angle(Vec3::new(0.0, 0.0, 1.0), err * k);
        self.orientation = (self.orientation * delta).normalized();
    }

    /// Follows the stick with a limited angular acceleration, then rotates
    /// about the body axes. Right-multiplying applies the increment in the
    /// body frame, which is what makes the rotation order-independent.
    fn turn(&mut self, stick: Vec3) {
        let target = stick * MAX_TURN_RATE;
        let max = TURN_ACCEL * STEP_DT;
        let follow = |cur: f32, want: f32| cur + (want - cur).clamp(-max, max);
        self.ang_vel = Vec3::new(
            follow(self.ang_vel.x, target.x),
            follow(self.ang_vel.y, target.y),
            follow(self.ang_vel.z, target.z),
        );
        let angle = self.ang_vel.length() * STEP_DT;
        let delta = Quat::from_axis_angle(self.ang_vel, angle);
        // Renormalizing every step stops rounding drift from accumulating.
        self.orientation = (self.orientation * delta).normalized();
    }
}
