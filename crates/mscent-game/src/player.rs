//! The 6DoF player (M6.1): thrust on three axes, pitch, yaw and roll held as
//! a quaternion, and inertia on both motion and rotation. It flies like a
//! big drone: thrust ramps in and out, the hull bobs while hovering, banks
//! into turns and strafes, and a free roll settles on the nearest quarter turn.
//!
//! Orientation is a unit quaternion updated by body-frame increments, so no
//! pitch angle is singular: looping through 90 degrees up behaves like any
//! other angle. Motion is the `mscent_collide` fixed step; `Player::step`
//! is called once per `STEP_DT` and nothing here reads a clock.

use crate::input::Input;
use mscent_collide::{Body, STEP_DT, Solids};
use mscent_math::{Quat, Vec3};

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
pub const STEER: f32 = 3.0;
/// How fast the applied thrust follows the stick, in stick units per second:
/// the drone spools up and down instead of snapping.
pub const THRUST_RAMP: f32 = 9.0;
/// How fast a free roll settles on a quarter turn, per second (proportional).
pub const LEVEL_RATE: f32 = 3.0;
/// Hover bob: peak height in metres, angular rate in radians per second, and
/// the speed above which it fades out completely.
pub const BOB_AMPLITUDE: f32 = 0.07;
pub const BOB_RATE: f32 = 2.2;
pub const BOB_FADE_SPEED: f32 = 3.0;
/// Banking: radians per radian/second of turn and per m/s of strafe, the
/// cap, and how fast the hull follows its target, per second.
pub const BANK_PER_TURN: f32 = 0.16;
pub const BANK_PER_STRAFE: f32 = 0.03;
pub const MAX_BANK: f32 = 0.4;
pub const BANK_FOLLOW: f32 = 5.0;
/// Smoothing of the mouse-turn rate estimate used for banking, per step.
const LOOK_SMOOTHING: f32 = 0.05;
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
    /// Size against the stock ship (the level's `scale`): the collision sphere,
    /// the thrust and so the top speed, and the hover bob all grow with it, so a
    /// big ship handles as the stock one does, measured in its own lengths.
    pub scale: f32,
    /// Thrust actually applied, in the body frame; follows the stick.
    thrust_applied: Vec3,
    /// Phase of the hover bob, radians.
    bob_phase: f32,
    /// Cosmetic roll into turns, radians (positive banks left).
    bank: f32,
    /// Yaw from mouse look since the last step, and its smoothed rate.
    look_kick: f32,
    look_rate: f32,
}

/// Moves `cur` toward `want` by at most `max` in length.
fn approach(cur: Vec3, want: Vec3, max: f32) -> Vec3 {
    let diff = want - cur;
    let len = diff.length();
    if len <= max {
        want
    } else {
        cur + diff * (max / len)
    }
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
            scale: 1.0,
            thrust_applied: Vec3::ZERO,
            bob_phase: 0.0,
            bank: 0.0,
            look_kick: 0.0,
            look_rate: 0.0,
        }
    }

    /// The same ship `scale` times its stock size. A scale that is not finite
    /// and positive leaves it as it is.
    pub fn scaled(mut self, scale: f32) -> Self {
        if scale.is_finite() && scale > 0.0 {
            self.scale = scale;
            self.body.radius = PLAYER_RADIUS * scale;
        }
        self
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
    pub fn step<'a>(&mut self, solids: impl Into<Solids<'a>>, input: &Input) {
        let i = input.sanitized();
        self.turn(Vec3::new(i.pitch, i.yaw, i.roll));
        // Forward on the stick is -Z in the body frame.
        let want = Vec3::new(i.thrust.x, i.thrust.y, -i.thrust.z);
        self.thrust_applied = approach(self.thrust_applied, want, THRUST_RAMP * STEP_DT);
        let local = self.thrust_applied;
        let thrust = self.orientation.rotate(local) * (THRUST_ACCEL * self.scale);
        let vel = self.body.vel;
        let mut drag = vel * DRAG;
        if want.length() < 1e-3 {
            drag = drag + vel * BRAKE;
        } else {
            let dir = thrust * (1.0 / THRUST_ACCEL);
            let dir = dir * (1.0 / dir.length().max(1e-6));
            drag = drag + (vel - dir * vel.dot(dir)) * STEER;
        }
        self.body.step(solids, thrust - drag);
        self.animate();
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
        self.look_kick += yaw;
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

    /// Advances the hover bob and the bank toward what the motion asks for.
    fn animate(&mut self) {
        self.bob_phase = (self.bob_phase + BOB_RATE * STEP_DT) % std::f32::consts::TAU;
        let kick_rate = std::mem::take(&mut self.look_kick) / STEP_DT;
        self.look_rate += (kick_rate - self.look_rate) * LOOK_SMOOTHING;
        let strafe = self.orientation.conjugate().rotate(self.body.vel).x;
        let target = ((self.ang_vel.y + self.look_rate) * BANK_PER_TURN - strafe * BANK_PER_STRAFE)
            .clamp(-MAX_BANK, MAX_BANK);
        self.bank += (target - self.bank) * (BANK_FOLLOW * STEP_DT).min(1.0);
    }

    /// Vertical hover bob for the camera. It fades out as the drone speeds
    /// up, and is cosmetic only: collision and aim ignore it.
    pub fn bob_offset(&self) -> Vec3 {
        let fade = BOB_FADE_SPEED * self.scale;
        let still = (1.0 - self.body.vel.length() / fade).clamp(0.0, 1.0);
        let up = self.up_ref.unwrap_or_else(|| self.up());
        up * (BOB_AMPLITUDE * self.scale * still * self.bob_phase.sin())
    }

    /// Cosmetic bank, applied to the camera about the view axis.
    pub fn bank_rotation(&self) -> Quat {
        Quat::from_axis_angle(Vec3::new(0.0, 0.0, 1.0), self.bank)
    }

    /// Eases the roll to the nearest quarter turn about the view axis, so a
    /// released roll locks onto upright, on its side or inverted.
    fn level(&mut self, world_up: Vec3) {
        let roll = self.right().dot(world_up).atan2(self.up().dot(world_up));
        let quarter = std::f32::consts::FRAC_PI_2;
        let err = (roll / quarter).round() * quarter - roll;
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
