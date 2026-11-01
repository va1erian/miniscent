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
pub const THRUST_ACCEL: f32 = 40.0;
/// Linear drag per second; terminal speed is `THRUST_ACCEL / DRAG`.
pub const DRAG: f32 = 2.0;
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
        self.body.step(bsp, thrust - self.body.vel * DRAG);
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
