//! The pilot on foot: a person who walks, runs, jumps and flies a short way on
//! a jetpack, with the ground and air control of a Quake-like shooter. The
//! level is Z-up and its units are the pilot's metres.
//!
//! The body is a stack of two spheres of [`PILOT_RADIUS`]: the feet, which
//! `body` moves and the ground holds up, and the head [`HEAD_RISE`] above them,
//! which a move may not push into a ceiling or a low wall. A step no higher
//! than [`STEP_HEIGHT`] is climbed without a jump. Like the ship, it is one
//! fixed step at a time and reads no clock.

use crate::input::Input;
use mscent_collide::{Body, STEP_DT, Solids, Trace, slide_move, sweep_sphere};
use mscent_math::epsilon::{GROUND_PROBE, MIN_LENGTH_F32, STEP_GAIN};
use mscent_math::{Quat, Vec3};

/// Height from sole to crown, metres.
pub const PILOT_HEIGHT: f32 = 1.8;
/// Radius of both spheres: shoulders 0.7 m across.
pub const PILOT_RADIUS: f32 = 0.35;
/// From the feet sphere's centre up to the head sphere's.
pub const HEAD_RISE: f32 = PILOT_HEIGHT - 2.0 * PILOT_RADIUS;
/// From the feet sphere's centre up to the eyes (1.65 m above the soles).
pub const EYE_RISE: f32 = 1.65 - PILOT_RADIUS;
/// Running speed on the ground, metres per second.
pub const RUN_SPEED: f32 = 6.0;
/// Ground acceleration, per second, times the wanted speed (Quake's form).
pub const GROUND_ACCEL: f32 = 10.0;
/// Ground friction per second, and the speed below which it acts as if the
/// pilot were going at this one, so a slow drift stops in a few steps.
pub const FRICTION: f32 = 6.0;
pub const STOP_SPEED: f32 = 2.5;
/// Air acceleration, and the most speed a wish adds along itself in the air:
/// little, so a jump keeps its line, but turning while strafing gains speed.
pub const AIR_ACCEL: f32 = 10.0;
pub const AIR_WISH_CAP: f32 = 0.9;
/// Gravity, metres per second squared, along -Z.
pub const GRAVITY: f32 = 18.0;
/// Speed up off the ground: a jump of `JUMP_SPEED^2 / (2 GRAVITY)`, 1.0 m.
pub const JUMP_SPEED: f32 = 6.0;
/// The steepest ground the pilot stands on: the least `normal.z`, about 46 degrees.
pub const WALKABLE_Z: f32 = 0.7;
/// The tallest step climbed without a jump.
pub const STEP_HEIGHT: f32 = 0.45;
/// Jetpack thrust, metres per second squared upward on top of gravity's pull,
/// and the climb speed past which it gives no more.
pub const JET_ACCEL: f32 = 30.0;
pub const JET_CLIMB: f32 = 7.0;
/// Below this upward speed a held jump lights the jetpack, so a tap is a plain
/// jump and holding on past the top of one flies; once lit it burns while held.
pub const JET_ENGAGE: f32 = 2.0;
/// How hard the jetpack steers sideways, as ground acceleration does.
pub const JET_STEER: f32 = 3.0;
/// Seconds of jetpack a full tank holds, and seconds refilled per second on
/// the ground.
pub const JET_FUEL: f32 = 1.5;
pub const JET_REFILL: f32 = 0.75;
/// The steepest look up or down, radians from level.
const MAX_PITCH: f32 = 1.5;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pilot {
    /// The feet sphere.
    pub body: Body,
    /// The view: body -Z forward, +Y up, as the ship's.
    pub orientation: Quat,
    pub grounded: bool,
    /// Seconds of jetpack left.
    pub fuel: f32,
    /// The jetpack fired this step, for the HUD and effects.
    pub jetting: bool,
    /// The jump control was held last step, so holding it jumps once.
    jump_held: bool,
}

/// `v` with its part along unit `n` taken out.
fn along_plane(v: Vec3, n: Vec3) -> Vec3 {
    v - n * v.dot(n)
}

fn flat(v: Vec3) -> Vec3 {
    Vec3::new(v.x, v.y, 0.0)
}

/// Quake's acceleration: adds speed along unit `dir` up to `cap`, at most
/// `accel * wish * dt` a step.
fn accelerate(vel: Vec3, dir: Vec3, (wish, cap): (f32, f32), accel: f32) -> Vec3 {
    let add = cap.min(wish) - vel.dot(dir);
    if add <= 0.0 {
        return vel;
    }
    vel + dir * (accel * wish * STEP_DT).min(add)
}

impl Pilot {
    /// Standing with the feet sphere's centre at `feet`, looking along the
    /// level heading of `look`, with a full tank.
    pub fn new(feet: Vec3, look: Quat) -> Self {
        Self {
            body: Body::new(feet, PILOT_RADIUS),
            orientation: heading_of(look),
            grounded: false,
            fuel: JET_FUEL,
            jetting: false,
            jump_held: false,
        }
    }

    pub fn pos(&self) -> Vec3 {
        self.body.pos
    }

    /// Where the eyes are: shots leave from here and the camera sits here.
    pub fn eye(&self) -> Vec3 {
        self.body.pos + Vec3::new(0.0, 0.0, EYE_RISE)
    }

    /// The middle of the body, which enemies aim at.
    pub fn chest(&self) -> Vec3 {
        self.body.pos + Vec3::new(0.0, 0.0, HEAD_RISE * 0.5)
    }

    /// Turns to look along `dir`, upright, within the pitch limit.
    pub fn face(&mut self, dir: Vec3) {
        let ahead = if flat(dir).length() > MIN_LENGTH_F32 {
            dir
        } else {
            self.forward()
        };
        let heading = (-ahead.x).atan2(ahead.y);
        let yaw = Quat::from_axis_angle(Vec3::new(0.0, 0.0, 1.0), heading);
        self.orientation = (yaw * upright()).normalized();
        self.look(dir.z.clamp(-1.0, 1.0).asin(), 0.0);
    }

    pub fn forward(&self) -> Vec3 {
        self.orientation.rotate(Vec3::new(0.0, 0.0, -1.0))
    }

    /// Mouse look: `yaw` about the world's up, `pitch` about the view's right,
    /// held short of straight up or down so the view never rolls over.
    pub fn look(&mut self, pitch: f32, yaw: f32) {
        if !(pitch.is_finite() && yaw.is_finite()) {
            return;
        }
        let now = self.forward().z.clamp(-1.0, 1.0).asin();
        let pitch = (now + pitch).clamp(-MAX_PITCH, MAX_PITCH) - now;
        let yaw_q = Quat::from_axis_angle(Vec3::new(0.0, 0.0, 1.0), yaw);
        let pitch_q = Quat::from_axis_angle(Vec3::new(1.0, 0.0, 0.0), pitch);
        self.orientation = (yaw_q * self.orientation * pitch_q).normalized();
    }

    /// One fixed step. Stick forward and sideways walk; up (or the jump key)
    /// jumps, and held in the air fires the jetpack.
    pub fn step<'a>(&mut self, solids: impl Into<Solids<'a>>, input: &Input) {
        let solids = solids.into();
        let i = input.sanitized();
        let wish = self.wish(i.thrust);
        let jump = i.thrust.y > 0.0;
        let mut vel = self.body.vel;
        let was_jetting = std::mem::take(&mut self.jetting);
        if self.grounded {
            vel = friction(vel);
            vel = accelerate(vel, wish.0, (wish.1, wish.1), GROUND_ACCEL);
            if jump && !self.jump_held {
                vel.z = JUMP_SPEED;
                self.grounded = false;
            }
            self.fuel = (self.fuel + JET_REFILL * STEP_DT).min(JET_FUEL);
        } else {
            vel = accelerate(vel, wish.0, (wish.1, AIR_WISH_CAP), AIR_ACCEL);
            vel.z -= GRAVITY * STEP_DT;
            if jump && self.fuel > 0.0 && (was_jetting || vel.z < JET_ENGAGE) {
                vel = self.jet(vel, wish);
            }
        }
        self.jump_held = jump;
        self.body.vel = vel;
        let climbed = self.travel(solids);
        self.find_ground(solids, climbed);
    }

    /// The jetpack's push for one step: up toward its climb speed, and steering.
    fn jet(&mut self, vel: Vec3, wish: (Vec3, f32)) -> Vec3 {
        self.jetting = true;
        self.fuel = (self.fuel - STEP_DT).max(0.0);
        let mut vel = accelerate(vel, wish.0, (wish.1, wish.1), JET_STEER);
        vel.z = (vel.z + JET_ACCEL * STEP_DT).min(JET_CLIMB.max(vel.z));
        vel
    }

    /// Unit direction and speed the stick asks for, level on the ground.
    fn wish(&self, stick: Vec3) -> (Vec3, f32) {
        let ahead = flat(self.forward());
        let right = flat(self.orientation.rotate(Vec3::new(1.0, 0.0, 0.0)));
        let want = ahead * stick.z + right * stick.x;
        match want.normalized() {
            Some(dir) if want.length() > MIN_LENGTH_F32 => {
                (dir, RUN_SPEED * want.length().min(1.0))
            }
            _ => (Vec3::ZERO, 0.0),
        }
    }

    /// Moves the feet: a plain slide, or on the ground the same move lifted
    /// over a step if that goes farther. The head must fit where they end.
    /// True when it climbed a step.
    fn travel(&mut self, solids: Solids) -> bool {
        let start = self.body.pos;
        self.body.prev_pos = start;
        let slid = slide_move(solids, start, self.body.vel, STEP_DT, PILOT_RADIUS);
        let mut end = (slid.pos, slid.vel);
        let mut climbed = false;
        if self.grounded
            && let Some(stepped) = self.stepped(solids, start)
            && flat(stepped.0 - start).length() > flat(slid.pos - start).length() + STEP_GAIN
        {
            end = stepped;
            climbed = true;
        }
        if self.grounded {
            // Riding up a step's rounded edge is not a jump.
            end.1.z = end.1.z.min(0.0);
            end.0 = self.snap_down(solids, end.0);
        }
        self.body.pos = end.0;
        self.body.vel = end.1;
        self.fit_head(solids, start);
        climbed
    }

    /// The move made from [`STEP_HEIGHT`] up and set down again, if it climbs
    /// onto ground the pilot can stand on. The sphere first sets down on the
    /// step's edge, which is too steep to stand on, so the ground is looked for
    /// a radius further on, where the feet are heading.
    fn stepped(&self, solids: Solids, start: Vec3) -> Option<(Vec3, Vec3)> {
        let lift = Vec3::new(0.0, 0.0, STEP_HEIGHT);
        let up = sweep_sphere(solids, start, start + lift, PILOT_RADIUS);
        if up.start_solid || !self.head_fits(solids, up.end) {
            return None;
        }
        let level = flat(self.body.vel);
        let moved = slide_move(solids, up.end, level, STEP_DT, PILOT_RADIUS);
        let drop = Vec3::new(0.0, 0.0, up.end.z - start.z + GROUND_PROBE);
        let down = sweep_sphere(solids, moved.pos, moved.pos - drop, PILOT_RADIUS);
        let climbed = down.hit && !down.start_solid && down.end.z > start.z + STEP_GAIN;
        let reach = level.normalized()? * PILOT_RADIUS;
        let ahead = sweep_sphere(solids, moved.pos, moved.pos + reach, PILOT_RADIUS).end;
        let floor = sweep_sphere(solids, ahead, ahead - drop, PILOT_RADIUS);
        let stands = floor.hit && !floor.start_solid && floor.normal.z >= WALKABLE_Z;
        (climbed && stands).then_some((down.end, Vec3::new(moved.vel.x, moved.vel.y, 0.0)))
    }

    /// Keeps a walker on a slope or down a step instead of flying off it.
    fn snap_down(&self, solids: Solids, at: Vec3) -> Vec3 {
        if self.body.vel.z > 0.0 {
            return at;
        }
        let below = Vec3::new(0.0, 0.0, STEP_HEIGHT);
        let down = sweep_sphere(solids, at, at - below, PILOT_RADIUS);
        if down.hit && !down.start_solid && down.normal.z >= WALKABLE_Z {
            down.end
        } else {
            at
        }
    }

    fn head_fits(&self, solids: Solids, feet: Vec3) -> bool {
        let rise = Vec3::new(0.0, 0.0, HEAD_RISE);
        !sweep_sphere(solids, feet, feet + rise, PILOT_RADIUS).hit
    }

    /// Undoes what of the move put the head into something: the climb, and if
    /// that is not enough, the whole move. A head already stuck stays as it is.
    fn fit_head(&mut self, solids: Solids, start: Vec3) {
        let end = self.body.pos;
        if self.head_fits(solids, end) || !self.head_fits(solids, start) {
            return;
        }
        self.body.vel.z = self.body.vel.z.min(0.0);
        let level = Vec3::new(end.x, end.y, start.z.min(end.z));
        let low_enough = !sweep_sphere(solids, start, level, PILOT_RADIUS).hit;
        if low_enough && self.head_fits(solids, level) {
            self.body.pos = level;
        } else {
            self.body.pos = start;
            self.body.vel = Vec3::new(0.0, 0.0, self.body.vel.z);
        }
    }

    /// Stands on what is just under the feet, if it is not too steep (or the
    /// feet are on a step's edge, `climbing`), and takes out the part of the
    /// velocity that goes into it.
    fn find_ground(&mut self, solids: Solids, climbing: bool) {
        let p = self.body.pos;
        let probe = Vec3::new(0.0, 0.0, GROUND_PROBE);
        let down: Trace = sweep_sphere(solids, p, p - probe, PILOT_RADIUS);
        let rising = self.body.vel.z > JET_ENGAGE;
        let walkable = down.normal.z >= WALKABLE_Z;
        self.grounded = down.hit && (walkable || climbing) && !rising;
        if self.grounded && walkable {
            self.body.vel = along_plane(self.body.vel, down.normal);
        }
    }
}

/// Ground friction for one step, on the level part of `vel` only.
fn friction(vel: Vec3) -> Vec3 {
    let speed = flat(vel).length();
    if speed < MIN_LENGTH_F32 {
        return Vec3::new(0.0, 0.0, vel.z);
    }
    let drop = speed.max(STOP_SPEED) * FRICTION * STEP_DT;
    let k = (speed - drop).max(0.0) / speed;
    Vec3::new(vel.x * k, vel.y * k, vel.z)
}

/// A view looking level along the heading of `look`, upright about +Z. A view
/// pointing straight up or down keeps the heading of its top instead.
pub fn heading_of(look: Quat) -> Quat {
    let fwd = look.rotate(Vec3::new(0.0, 0.0, -1.0));
    let top = look.rotate(Vec3::new(0.0, 1.0, 0.0));
    let ahead = if flat(fwd).length() > 0.1 { fwd } else { top };
    let heading = (-ahead.x).atan2(ahead.y);
    (Quat::from_axis_angle(Vec3::new(0.0, 0.0, 1.0), heading) * upright()).normalized()
}

/// Turns body -Z forward and +Y up into world +Y forward and +Z up.
fn upright() -> Quat {
    Quat::from_axis_angle(Vec3::new(1.0, 0.0, 0.0), core::f32::consts::FRAC_PI_2)
}
