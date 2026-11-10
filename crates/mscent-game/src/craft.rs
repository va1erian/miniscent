//! Leaving the ship and boarding it again, and the one body the player is
//! playing at any moment: the ship, or the pilot on foot while the ship stands
//! parked where it landed. Any level allows both; what fits where is the level's
//! geometry, measured with the scale guide (`scale.rs`).
//!
//! Enemies, zones, pickups, the use key, shots and the goal all follow that
//! body, so a level can send the pilot through a door the ship cannot take.

use crate::game::{Event, Game};
use crate::input::Input;
use crate::pilot::{HEAD_RISE, PILOT_HEIGHT, PILOT_RADIUS, Pilot, WALKABLE_Z, heading_of};
use mscent_collide::{Solids, sweep_sphere};
use mscent_math::{Quat, Vec3};

/// The ship counts as landed below this speed, in stock-ship units per second
/// (it grows with the ship's scale)...
pub const LANDED_SPEED: f32 = 1.5;
/// ...with ground no farther under its sphere than this share of its radius.
pub const LANDED_GAP: f32 = 1.0;
/// How far from the ship's hull the pilot may be and still climb aboard, metres.
pub const BOARD_REACH: f32 = 2.5;
/// Gap left between the hull and the pilot stepping out, metres.
const EXIT_GAP: f32 = 0.4;
/// How far down from the ship's centre to look for ground to step out on, in
/// ship radii.
const EXIT_DROP: f32 = 3.0;

impl Game {
    /// True while the pilot is out of the ship.
    pub fn on_foot(&self) -> bool {
        self.pilot.is_some()
    }

    /// Where the body being played is: the ship's centre, or the pilot's chest.
    pub fn actor_pos(&self) -> Vec3 {
        self.pilot.as_ref().map_or(self.player.pos(), Pilot::chest)
    }

    /// The radius of the sphere that stands for the body being played, for
    /// enemy fire, touching things and reaching the goal.
    pub fn actor_radius(&self) -> f32 {
        match self.pilot {
            Some(_) => PILOT_HEIGHT * 0.5,
            None => self.player.body.radius,
        }
    }

    /// Where shots and the use key reach from, and which way.
    pub fn aim(&self) -> (Vec3, Vec3) {
        match &self.pilot {
            Some(p) => (p.eye(), p.forward()),
            None => (self.player.pos(), self.player.forward()),
        }
    }

    /// Which way the played body looks: the camera's turn before anything
    /// cosmetic.
    pub fn view_orientation(&self) -> Quat {
        self.pilot
            .as_ref()
            .map_or(self.player.orientation, |p| p.orientation)
    }

    /// The played body's right, for panning sound.
    pub fn actor_right(&self) -> Vec3 {
        match &self.pilot {
            Some(p) => p.orientation.rotate(Vec3::new(1.0, 0.0, 0.0)),
            None => self.player.right(),
        }
    }

    /// Mouse look for whichever body is played.
    pub fn look(&mut self, pitch: f32, yaw: f32) {
        match &mut self.pilot {
            Some(p) => p.look(pitch, yaw),
            None => self.player.look(pitch, yaw),
        }
    }

    /// Whether the ship is down: slow, with walkable ground just under it.
    pub fn ship_landed(&self) -> bool {
        let ship = &self.player;
        if ship.body.vel.length() > LANDED_SPEED * ship.scale {
            return false;
        }
        let r = ship.body.radius;
        let below = Vec3::new(0.0, 0.0, r * LANDED_GAP);
        let solids = self.arena.solids(&self.solids);
        let down = sweep_sphere(solids, ship.pos(), ship.pos() - below, r);
        down.hit && !down.start_solid && down.normal.z >= WALKABLE_Z
    }

    /// Whether the pilot is close enough to the parked ship to climb in.
    pub fn can_board(&self) -> bool {
        self.pilot.as_ref().is_some_and(|p| {
            let reach = self.player.body.radius + BOARD_REACH + PILOT_RADIUS;
            (p.chest() - self.player.pos()).length() <= reach
        })
    }

    /// One step of the board control: a fresh press leaves or boards.
    pub(crate) fn board_control(&mut self, input: &Input) {
        let pressed = input.board && !self.board_held;
        self.board_held = input.board;
        if !pressed {
            return;
        }
        if self.pilot.is_some() {
            self.board();
        } else {
            self.disembark();
        }
    }

    /// Climbs back in: the ship flies again from where it stands.
    pub(crate) fn board(&mut self) {
        if !self.can_board() {
            self.events.push(Event::BoardRefused);
            return;
        }
        self.pilot = None;
        self.events.push(Event::Boarded);
    }

    /// Steps out of a landed ship, beside it on the ground, and parks the ship.
    pub(crate) fn disembark(&mut self) {
        let spot = self.ship_landed().then(|| self.exit_spot()).flatten();
        let Some(feet) = spot else {
            self.events.push(Event::BoardRefused);
            return;
        };
        self.park_ship();
        self.pilot = Some(Pilot::new(feet, self.player.orientation));
        self.events.push(Event::Disembarked);
    }

    /// Sets the ship down on the ground under it and stops it.
    fn park_ship(&mut self) {
        let ship = &mut self.player;
        let r = ship.body.radius;
        let solids = self.arena.solids(&self.solids);
        let at = ship.body.pos;
        let down = sweep_sphere(solids, at, at - Vec3::new(0.0, 0.0, r * LANDED_GAP), r);
        if !down.start_solid {
            ship.body.pos = down.end;
        }
        ship.body.prev_pos = ship.body.pos;
        ship.body.vel = Vec3::ZERO;
    }

    /// Where the pilot's feet go on leaving: on the ground to the ship's right,
    /// left, back or front, the first with room to stand and a clear way out.
    fn exit_spot(&self) -> Option<Vec3> {
        let ship = &self.player;
        let solids = self.arena.solids(&self.solids);
        let out = ship.body.radius + PILOT_RADIUS + EXIT_GAP;
        let level = heading_of(ship.orientation);
        let right = level.rotate(Vec3::new(1.0, 0.0, 0.0));
        let ahead = level.rotate(Vec3::new(0.0, 0.0, -1.0));
        [right, -right, -ahead, ahead]
            .into_iter()
            .find_map(|dir| stand_at(solids, ship.pos(), ship.pos() + dir * out, ship.body.radius))
    }
}

/// The feet over `beside` on walkable ground, if the pilot can get there from
/// `from` and stand there.
fn stand_at(solids: Solids, from: Vec3, beside: Vec3, ship_radius: f32) -> Option<Vec3> {
    let way = sweep_sphere(solids, from, beside, PILOT_RADIUS);
    if way.hit || way.start_solid {
        return None;
    }
    let drop = Vec3::new(0.0, 0.0, ship_radius * EXIT_DROP);
    let down = sweep_sphere(solids, beside, beside - drop, PILOT_RADIUS);
    if !down.hit || down.start_solid || down.normal.z < WALKABLE_Z {
        return None;
    }
    let head = sweep_sphere(
        solids,
        down.end,
        down.end + Vec3::new(0.0, 0.0, HEAD_RISE),
        PILOT_RADIUS,
    );
    (!head.hit).then_some(down.end)
}
