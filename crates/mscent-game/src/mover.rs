//! Movers at play (M10.3): named poses, eased motion and what happens when a
//! body is in the way. Doors, lifts, trapdoors and crushers are all this.
//!
//! Collision and pushing are in `mscent_collide`; this decides where each
//! mover wants to be next. Movers run after the bodies in a step, so a push
//! lands between a body's two interpolated positions and draws smoothly.

use crate::game::{Event, Game, HURT_FLASH_STEPS};
use mscent_collide::{Body, MoverSolid, Pose, Push, STEP_DT, STEP_HZ, push_mover};
use mscent_math::Vec3d;
use mscent_world::entity::MAX_ENTITY_COORD;
use std::sync::Arc;

/// The pose every mover has: as it was authored.
pub const CLOSED: &str = "closed";
/// Most named poses on one mover.
pub const MAX_POSES: usize = 16;
/// Longest move, so a hostile time cannot make a mover's step round to nothing.
pub const MAX_MOVE_SECONDS: f32 = 3600.0;

/// What a mover does when a body has nowhere to go.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum WhenBlocked {
    /// Stay put and try again next step.
    Hold,
    /// Go back the way it came.
    Reverse,
    /// Stay put and hurt the body until it is gone.
    Crush { damage_per_second: f32 },
}

/// Why a request to a mover was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MoverError {
    NoSuchMover,
    NoSuchPose,
    /// More than [`MAX_POSES`], or a redefinition of [`CLOSED`].
    BadPoseName,
    /// Non-finite, or farther than any level coordinate.
    BadPose,
    /// Negative, non-finite or over [`MAX_MOVE_SECONDS`].
    BadTime,
    /// Negative or non-finite.
    BadDamage,
}

#[derive(Clone, Debug, PartialEq)]
struct Motion {
    from: Pose,
    to: Pose,
    /// The named poses at each end. An interrupted move starts between poses.
    from_pose: Option<usize>,
    to_pose: Option<usize>,
    /// Steps taken of the total. Counted, not summed as a fraction, so a move
    /// takes exactly as long as it was asked to.
    step: u32,
    steps: u32,
}

/// One mover's state.
#[derive(Clone, Debug, PartialEq)]
pub struct Mover {
    poses: Vec<(String, Pose)>,
    current: Pose,
    /// Where it was one step ago, for drawing between the two.
    previous: Pose,
    at: Option<usize>,
    motion: Option<Motion>,
    when_blocked: WhenBlocked,
    /// Whether the last step was blocked, so the event fires once per stoppage.
    blocked: bool,
    /// Crush damage owed but below one whole point.
    crush_owed: f32,
}

fn eased(s: f32) -> f32 {
    s * s * (3.0 - 2.0 * s)
}

fn pose_in_range(pose: &Pose) -> bool {
    let limit = f64::from(MAX_ENTITY_COORD);
    let fits = |v: Vec3d| v.x.abs() <= limit && v.y.abs() <= limit && v.z.abs() <= limit;
    pose.is_finite() && fits(pose.pivot) && fits(pose.offset)
}

impl Mover {
    fn new() -> Self {
        Self {
            poses: vec![(CLOSED.to_string(), Pose::IDENTITY)],
            current: Pose::IDENTITY,
            previous: Pose::IDENTITY,
            at: Some(0),
            motion: None,
            when_blocked: WhenBlocked::Reverse,
            blocked: false,
            crush_owed: 0.0,
        }
    }

    /// The pose it is resting at or last reached, if it is a named one.
    pub fn pose_name(&self) -> Option<&str> {
        let i = self.at?;
        self.poses.get(i).map(|(name, _)| name.as_str())
    }

    /// The named pose it is on its way to, if it is moving to one.
    pub fn target_name(&self) -> Option<&str> {
        let i = self.motion.as_ref()?.to_pose?;
        self.poses.get(i).map(|(name, _)| name.as_str())
    }

    pub fn is_moving(&self) -> bool {
        self.motion.is_some()
    }

    pub fn pose(&self) -> &Pose {
        &self.current
    }

    /// Where to draw it, a fraction `alpha` of the way through the last step.
    pub fn drawn_pose(&self, alpha: f32) -> Pose {
        Pose::between(&self.previous, &self.current, alpha)
    }

    /// The next pose if it is moving.
    fn next(&self) -> Option<Pose> {
        let m = self.motion.as_ref()?;
        let s = (m.step + 1) as f32 / m.steps as f32;
        Some(Pose::between(&m.from, &m.to, eased(s)))
    }

    /// Commits a step; true when it has arrived.
    fn advance(&mut self, pose: Pose) -> bool {
        self.current = pose;
        self.blocked = false;
        let Some(m) = self.motion.as_mut() else {
            return false;
        };
        m.step += 1;
        if m.step < m.steps {
            return false;
        }
        self.at = m.to_pose;
        self.motion = None;
        true
    }

    fn reverse(&mut self) {
        if let Some(m) = self.motion.as_mut() {
            core::mem::swap(&mut m.from, &mut m.to);
            core::mem::swap(&mut m.from_pose, &mut m.to_pose);
            m.step = m.steps - m.step;
        }
    }
}

/// The movers of a level, as authored.
pub(crate) fn spawn_movers(arena: &crate::Arena) -> (Vec<Mover>, Vec<MoverSolid>) {
    let solids = arena.movers.iter();
    let solids = solids.map(|m| MoverSolid::new(Arc::clone(&m.shape)));
    (
        arena.movers.iter().map(|_| Mover::new()).collect(),
        solids.collect(),
    )
}

impl Game {
    pub fn mover_index(&self, name: &str) -> Option<usize> {
        self.arena.movers.iter().position(|m| m.name == name)
    }

    pub fn mover(&self, index: usize) -> Option<&Mover> {
        self.movers.get(index)
    }

    /// Adds or replaces a named pose. [`CLOSED`] is fixed.
    pub fn define_pose(&mut self, index: usize, name: &str, pose: Pose) -> Result<(), MoverError> {
        let mover = self.movers.get_mut(index).ok_or(MoverError::NoSuchMover)?;
        if !pose_in_range(&pose) {
            return Err(MoverError::BadPose);
        }
        if name == CLOSED {
            return Err(MoverError::BadPoseName);
        }
        if let Some(slot) = mover.poses.iter_mut().find(|(n, _)| n == name) {
            slot.1 = pose;
            return Ok(());
        }
        if mover.poses.len() >= MAX_POSES {
            return Err(MoverError::BadPoseName);
        }
        mover.poses.push((name.to_string(), pose));
        Ok(())
    }

    /// Starts moving to a named pose, from wherever it is now, taking
    /// `seconds`. Zero is one step.
    pub fn move_mover(&mut self, index: usize, pose: &str, seconds: f32) -> Result<(), MoverError> {
        if !seconds.is_finite() || !(0.0..=MAX_MOVE_SECONDS).contains(&seconds) {
            return Err(MoverError::BadTime);
        }
        let mover = self.movers.get_mut(index).ok_or(MoverError::NoSuchMover)?;
        let found = mover.poses.iter().position(|(n, _)| n == pose);
        let to_pose = found.ok_or(MoverError::NoSuchPose)?;
        mover.motion = Some(Motion {
            from: mover.current,
            to: mover.poses[to_pose].1,
            from_pose: mover.at.filter(|_| mover.motion.is_none()),
            to_pose: Some(to_pose),
            step: 0,
            // `seconds` is capped, so this cannot overflow.
            steps: ((seconds * STEP_HZ as f32).round() as u32).max(1),
        });
        mover.blocked = false;
        self.events.push(Event::MoverSent(index));
        Ok(())
    }

    pub fn set_when_blocked(&mut self, index: usize, when: WhenBlocked) -> Result<(), MoverError> {
        if let WhenBlocked::Crush { damage_per_second } = when
            && !(damage_per_second.is_finite() && damage_per_second >= 0.0)
        {
            return Err(MoverError::BadDamage);
        }
        let mover = self.movers.get_mut(index).ok_or(MoverError::NoSuchMover)?;
        mover.when_blocked = when;
        Ok(())
    }

    pub(crate) fn run_movers(&mut self) {
        for i in 0..self.movers.len() {
            self.movers[i].previous = self.movers[i].current;
            let Some(pose) = self.movers[i].next() else {
                continue;
            };
            // The player first, then the living enemies, as `hurt_body` expects.
            let mut bodies: Vec<&mut Body> = vec![&mut self.player.body];
            let alive = self.enemies.iter_mut().filter(|e| e.alive());
            bodies.extend(alive.map(|e| &mut e.body));
            match push_mover(&self.arena.bsp, &mut self.solids, i, &pose, &mut bodies) {
                Push::Moved => {
                    if self.movers[i].advance(pose) {
                        self.events.push(Event::MoverArrived(i));
                    }
                }
                Push::Blocked(body) => self.mover_blocked(i, body),
            }
        }
    }

    fn mover_blocked(&mut self, index: usize, body: usize) {
        let mover = &mut self.movers[index];
        if !mover.blocked {
            self.events.push(Event::MoverBlocked(index));
        }
        mover.blocked = true;
        match mover.when_blocked {
            WhenBlocked::Hold => {}
            WhenBlocked::Reverse => mover.reverse(),
            WhenBlocked::Crush { damage_per_second } => {
                mover.crush_owed += damage_per_second * STEP_DT;
                let whole = mover.crush_owed.floor();
                mover.crush_owed -= whole;
                self.hurt_body(body, whole as i32);
            }
        }
    }

    /// `body` indexes the list built in [`Game::run_movers`].
    fn hurt_body(&mut self, body: usize, damage: i32) {
        if damage <= 0 {
            return;
        }
        let Some(enemy_nth) = body.checked_sub(1) else {
            self.player.damage(damage);
            self.hurt_steps = HURT_FLASH_STEPS;
            self.events.push(Event::PlayerHurt(damage));
            return;
        };
        let mut alive = self
            .enemies
            .iter_mut()
            .enumerate()
            .filter(|(_, e)| e.alive());
        if let Some((i, enemy)) = alive.nth(enemy_nth) {
            self.events.push(Event::EnemyHit(i));
            if enemy.damage(damage) {
                self.events.push(Event::EnemyKilled(i));
            }
        }
    }
}
