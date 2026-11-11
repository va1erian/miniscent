//! Game functions only a test script gets (`Game::load_test_script`). They steer
//! the player the way a person would, without scripted mouse moves that break
//! whenever geometry moves.

use crate::combat::line_of_sight;
use crate::game::{Game, Phase};
use crate::script_api::{ENEMY, MARKER, MOVER, PROP, Res, ZONE, nil, number, one, point, string};
use mscent_collide::STEP_HZ;
use mscent_math::{Quat, Vec3};
use mscent_script::{Flow, Heap, Native, Value};

/// From the pilot's feet sphere up to the chest, which is where the pilot is.
const CHEST: Vec3 = Vec3::new(0.0, 0.0, crate::pilot::HEAD_RISE * 0.5);
/// Most failed or passed checks kept. A test in a loop should not fill memory.
const MAX_CHECKS: usize = 4096;
/// Most screenshots one test can ask for.
const MAX_SHOTS: usize = 256;
/// Longest screenshot name.
const MAX_SHOT_NAME: usize = 64;

/// Directions tried by `teleport:distance:`: the axes, then the diagonals.
const AROUND: [[f32; 3]; 14] = [
    [-1.0, 0.0, 0.0],
    [1.0, 0.0, 0.0],
    [0.0, -1.0, 0.0],
    [0.0, 1.0, 0.0],
    [0.0, 0.0, 1.0],
    [0.0, 0.0, -1.0],
    [-1.0, -1.0, 1.0],
    [1.0, -1.0, 1.0],
    [-1.0, 1.0, 1.0],
    [1.0, 1.0, 1.0],
    [-1.0, -1.0, -1.0],
    [1.0, -1.0, -1.0],
    [-1.0, 1.0, -1.0],
    [1.0, 1.0, -1.0],
];

/// One `assert:because:`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Check {
    pub passed: bool,
    pub because: String,
}

pub(crate) const NATIVES: &[(&str, Native<Game>)] = &[
    ("assert:because:", check),
    ("teleport:distance:", teleport),
    ("flyTo:", fly_to),
    ("aimAt:", aim_at),
    ("fireFor:", fire_for),
    ("useFor:", use_for),
    ("walkFor:", |g, h, _, a| {
        let (until, seconds) = hold_ticks(g, h, a, "walkFor:")?;
        g.script.walk_until = until;
        Ok(Flow::Wait(seconds))
    }),
    ("jumpFor:", |g, h, _, a| {
        let (until, seconds) = hold_ticks(g, h, a, "jumpFor:")?;
        g.script.jump_until = until;
        Ok(Flow::Wait(seconds))
    }),
    ("pressBoard", press_board),
    ("shot:", shot),
    ("phase", phase),
];

impl Game {
    /// What a test script is holding down: use this as the input of each step.
    pub fn test_input(&self) -> crate::Input {
        crate::Input {
            fire: self.ticks < self.script.fire_until,
            use_key: self.ticks < self.script.use_until,
            thrust: Vec3::new(
                0.0,
                f32::from(u8::from(self.ticks < self.script.jump_until)),
                f32::from(u8::from(self.ticks < self.script.walk_until)),
            ),
            board: self.ticks < self.script.board_until,
            ..crate::Input::default()
        }
    }

    /// The test's checks since the last call.
    pub fn take_checks(&mut self) -> Vec<Check> {
        std::mem::take(&mut self.script.checks)
    }

    /// The screenshots the test has asked for since the last call.
    pub fn take_shots(&mut self) -> Vec<String> {
        std::mem::take(&mut self.script.shots)
    }

    /// Where a thing is, for aiming. Everything a test can aim at has a place.
    fn place_of(&self, h: &Heap, v: Value) -> Result<Vec3, String> {
        match v {
            Value::Handle(ENEMY, i) => {
                let enemy = self.enemies.get(i as usize).ok_or("no such enemy")?;
                Ok(enemy.pos())
            }
            Value::Handle(MOVER, i) => {
                let solid = self.solids.get(i as usize).ok_or("no such mover")?;
                Ok(solid.centre().to_f32())
            }
            Value::Handle(ZONE, i) => {
                let zone = self.arena.zones.get(i as usize).ok_or("no such zone")?;
                Ok(zone.pos)
            }
            Value::Handle(PROP, i) => {
                let prop = self.props.get(i as usize).ok_or("no such prop")?;
                Ok(prop.pos)
            }
            Value::Handle(MARKER, _) => point(self, h, v),
            _ => point(self, h, v),
        }
    }
}

fn check(g: &mut Game, h: &mut Heap, _: Value, args: &[Value]) -> Res {
    let &[passed, because] = args else {
        return Err("assert:because: wants a condition and a reason".into());
    };
    let Value::Bool(passed) = passed else {
        return Err(format!(
            "assert: wants true or false, not {}",
            h.describe(passed)
        ));
    };
    if g.script.checks.len() < MAX_CHECKS {
        g.script.checks.push(Check {
            passed,
            because: h.text(because),
        });
    }
    nil()
}

/// Moves the body being played: the pilot's chest, or the ship's centre.
fn put_player(g: &mut Game, pos: Vec3) {
    let (body, pos) = match &mut g.pilot {
        Some(p) => {
            p.grounded = false;
            (&mut p.body, pos - CHEST)
        }
        None => (&mut g.player.body, pos),
    };
    body.pos = pos;
    body.prev_pos = pos;
    body.vel = Vec3::ZERO;
}

/// Presses the board control for a few steps, as a key press does: the pilot
/// leaves a landed ship, or boards it.
fn press_board(g: &mut Game, _: &mut Heap, _: Value, _: &[Value]) -> Res {
    const PRESS: f32 = 2.0 / STEP_HZ as f32;
    g.script.board_until = g.ticks.saturating_add(2);
    Ok(Flow::Wait(PRESS * 2.0))
}

/// Puts the player `distance` from a thing, in the first direction with room
/// to stand and a clear view of it.
fn teleport(g: &mut Game, h: &mut Heap, _: Value, args: &[Value]) -> Res {
    let &[target, distance] = args else {
        return Err("teleport:distance: wants a thing and a distance".into());
    };
    let (at, distance) = (g.place_of(h, target)?, number(h, distance, "distance:")?);
    let solids = g.arena.solids(&g.solids);
    let clear = AROUND.iter().find_map(|d| {
        let dir = Vec3::new(d[0], d[1], d[2]).normalized()?;
        let pos = at + dir * distance;
        let open = !g.arena.is_solid(pos.to_f64());
        (open && line_of_sight(solids, pos, at)).then_some(pos)
    });
    let pos = clear.ok_or_else(|| format!("no clear place {distance} from there"))?;
    put_player(g, pos);
    nil()
}

fn fly_to(g: &mut Game, h: &mut Heap, _: Value, args: &[Value]) -> Res {
    let pos = g.place_of(h, one(args)?)?;
    if g.arena.is_solid(pos.to_f64()) {
        return Err("flyTo: that place is inside solid rock".into());
    }
    put_player(g, pos);
    nil()
}

/// The shortest rotation that takes the player's forward, -Z, to `dir`.
fn looking_along(dir: Vec3) -> Quat {
    let forward = Vec3::new(0.0, 0.0, -1.0);
    let axis = forward.cross(dir);
    let w = 1.0 + forward.dot(dir);
    if w <= f32::EPSILON {
        return Quat::from_axis_angle(Vec3::new(0.0, 1.0, 0.0), core::f32::consts::PI);
    }
    Quat {
        x: axis.x,
        y: axis.y,
        z: axis.z,
        w,
    }
    .normalized()
}

fn aim_at(g: &mut Game, h: &mut Heap, _: Value, args: &[Value]) -> Res {
    let at = g.place_of(h, one(args)?)?;
    let (from, _) = g.aim();
    let dir = (at - from)
        .normalized()
        .ok_or("aimAt: the player is at that very place")?;
    match &mut g.pilot {
        Some(p) => p.face(dir),
        None => g.player.orientation = looking_along(dir),
    }
    nil()
}

fn hold_ticks(g: &Game, h: &Heap, args: &[Value], what: &str) -> Result<(u64, f32), String> {
    let seconds = number(h, one(args)?, what)?;
    if seconds < 0.0 {
        return Err(format!("{what} must not be negative"));
    }
    let until = g
        .ticks
        .saturating_add((seconds * STEP_HZ as f32).ceil() as u64);
    Ok((until, seconds))
}

fn fire_for(g: &mut Game, h: &mut Heap, _: Value, args: &[Value]) -> Res {
    let (until, seconds) = hold_ticks(g, h, args, "fireFor:")?;
    g.script.fire_until = until;
    Ok(Flow::Wait(seconds))
}

fn use_for(g: &mut Game, h: &mut Heap, _: Value, args: &[Value]) -> Res {
    let (until, seconds) = hold_ticks(g, h, args, "useFor:")?;
    g.script.use_until = until;
    Ok(Flow::Wait(seconds))
}

/// Only a plain file name: the run decides where screenshots go, not the script.
fn shot(g: &mut Game, h: &mut Heap, _: Value, args: &[Value]) -> Res {
    let name = string(h, one(args)?, "shot:")?;
    let plain = |c: char| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.');
    let safe = !name.is_empty()
        && name.len() <= MAX_SHOT_NAME
        && name.chars().all(plain)
        && !name.contains("..");
    if !safe {
        return Err(format!("shot: wants a plain file name, not '{name}'"));
    }
    if g.script.shots.len() >= MAX_SHOTS {
        return Err(format!("too many shots: the limit is {MAX_SHOTS}"));
    }
    g.script.shots.push(name);
    nil()
}

fn phase(g: &mut Game, h: &mut Heap, _: Value, _: &[Value]) -> Res {
    let name = match g.phase {
        Phase::Playing => "playing",
        Phase::Won => "won",
        Phase::Lost => "lost",
    };
    h.new_str(name.to_string()).map(Flow::Val)
}
