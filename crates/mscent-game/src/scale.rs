//! The scale guide: what a level must leave open for the ship and for the pilot
//! on foot, worked out from the same numbers the simulation runs on, so the
//! guide cannot drift from the game. Units are the pilot's metres in every
//! level; the ship is the level's `scale` times its stock size.
//!
//! `miniscent --scale-guide [level]` prints it, the editor draws the two bodies
//! at the player start from it, and a level test can hold a level to it.

use crate::craft::BOARD_REACH;
use crate::pilot::{
    GRAVITY, JET_ACCEL, JET_CLIMB, JET_FUEL, JUMP_SPEED, PILOT_HEIGHT, PILOT_RADIUS, RUN_SPEED,
    STEP_HEIGHT, WALKABLE_Z,
};
use crate::player::{DRAG, PLAYER_RADIUS, THRUST_ACCEL};
use std::fmt;

/// Room left around a body in a doorway it is meant to pass with ease, as a
/// share of its size. Tighter than this is a squeeze a player will snag on.
pub const COMFORT: f32 = 1.5;

/// Sizes for building a level whose ship is `ship_scale` times the stock one.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScaleGuide {
    pub ship_scale: f32,
    /// The ship's collision sphere.
    pub ship_radius: f32,
    /// The ship's top speed, metres per second.
    pub ship_speed: f32,
    /// The least opening the ship fits through at all, and one it flies through
    /// without care.
    pub ship_gap: f32,
    pub ship_door: f32,
    /// The pilot's size.
    pub pilot_height: f32,
    pub pilot_width: f32,
    /// The least door width and height the pilot fits, and an easy door.
    pub pilot_gap: (f32, f32),
    pub pilot_door: (f32, f32),
    pub run_speed: f32,
    /// Tallest step walked up, highest jump, and the most the jetpack lifts
    /// from a standing jump on a full tank.
    pub step: f32,
    pub jump: f32,
    pub jet_climb: f32,
    /// Steepest walkable slope, degrees.
    pub slope_degrees: f32,
    /// How near the ship the pilot must be to board it, from its hull.
    pub board_reach: f32,
}

impl ScaleGuide {
    /// The guide for a ship `ship_scale` times its stock size.
    pub fn new(ship_scale: f32) -> Self {
        let ship_radius = PLAYER_RADIUS * ship_scale;
        let pilot_width = 2.0 * PILOT_RADIUS;
        Self {
            ship_scale,
            ship_radius,
            ship_speed: THRUST_ACCEL * ship_scale / DRAG,
            ship_gap: 2.0 * ship_radius,
            ship_door: 2.0 * ship_radius * COMFORT,
            pilot_height: PILOT_HEIGHT,
            pilot_width,
            pilot_gap: (pilot_width, PILOT_HEIGHT),
            pilot_door: (pilot_width * COMFORT, PILOT_HEIGHT * COMFORT.sqrt()),
            run_speed: RUN_SPEED,
            step: STEP_HEIGHT,
            jump: JUMP_SPEED * JUMP_SPEED / (2.0 * GRAVITY),
            jet_climb: jet_climb(),
            slope_degrees: WALKABLE_Z.acos().to_degrees(),
            board_reach: BOARD_REACH,
        }
    }

    /// Whether an opening `width` by `height` lets the ship through.
    pub fn ship_fits(&self, width: f32, height: f32) -> bool {
        width.min(height) >= self.ship_gap
    }

    /// Whether an opening `width` by `height` lets the pilot through on foot.
    pub fn pilot_fits(&self, width: f32, height: f32) -> bool {
        width >= self.pilot_gap.0 && height >= self.pilot_gap.1
    }
}

/// Height gained from a standing jump with the jump held until the tank is
/// dry, then coasting to the top: the jump's rise to where the jetpack lights,
/// the climb under thrust, and the coast. Worked in closed form from the
/// constants, so it matches the stepped game to within a step's travel.
fn jet_climb() -> f32 {
    use crate::pilot::JET_ENGAGE;
    let rise_to_engage = (JUMP_SPEED * JUMP_SPEED - JET_ENGAGE * JET_ENGAGE) / (2.0 * GRAVITY);
    let net = JET_ACCEL - GRAVITY;
    // Speeding up from the engage speed to the climb cap, then holding it.
    let ramp = ((JET_CLIMB - JET_ENGAGE) / net).min(JET_FUEL);
    let ramp_rise = JET_ENGAGE * ramp + 0.5 * net * ramp * ramp;
    let top = JET_ENGAGE + net * ramp;
    let held = (JET_FUEL - ramp).max(0.0) * top;
    let coast = top * top / (2.0 * GRAVITY);
    rise_to_engage + ramp_rise + held + coast
}

impl fmt::Display for ScaleGuide {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(
            f,
            "Scale guide: units are metres; ship scale {}",
            self.ship_scale
        )?;
        writeln!(f, "Ship")?;
        writeln!(f, "  sphere radius        {:.2} m", self.ship_radius)?;
        writeln!(f, "  top speed            {:.1} m/s", self.ship_speed)?;
        writeln!(f, "  fits an opening of   {:.2} m", self.ship_gap)?;
        writeln!(f, "  easy opening         {:.2} m", self.ship_door)?;
        writeln!(f, "Pilot")?;
        writeln!(
            f,
            "  height x width       {:.2} x {:.2} m",
            self.pilot_height, self.pilot_width
        )?;
        writeln!(
            f,
            "  fits a door of       {:.2} wide, {:.2} high",
            self.pilot_gap.0, self.pilot_gap.1
        )?;
        writeln!(
            f,
            "  easy door            {:.2} wide, {:.2} high",
            self.pilot_door.0, self.pilot_door.1
        )?;
        writeln!(f, "  run speed            {:.1} m/s", self.run_speed)?;
        writeln!(
            f,
            "  step / jump          {:.2} / {:.2} m",
            self.step, self.jump
        )?;
        writeln!(
            f,
            "  jetpack climb        {:.1} m on a full tank",
            self.jet_climb
        )?;
        writeln!(
            f,
            "  steepest slope       {:.0} degrees",
            self.slope_degrees
        )?;
        writeln!(
            f,
            "  boards from          {:.1} m off the hull",
            self.board_reach
        )?;
        let (wide, high) = self.pilot_gap;
        if high < self.ship_gap {
            write!(
                f,
                "A door at least {wide:.2} wide and {high:.2} high, with its width or its                  height under {:.2} m, lets the pilot in and keeps the ship out.",
                self.ship_gap
            )
        } else {
            write!(
                f,
                "No door keeps this ship out and lets the pilot in: the ship is the smaller."
            )
        }
    }
}
