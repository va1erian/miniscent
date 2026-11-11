//! One tick of player intent, independent of any device.

use mscent_math::Vec3;

/// Player intent for one fixed step. Axes are in the body frame: thrust `x`
/// is right, `y` is up and `z` is forward. Rotation rates are right-handed
/// about the body axes: positive pitch lifts the nose, positive yaw turns
/// left and positive roll is a counter-clockwise turn about the view axis.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Input {
    pub thrust: Vec3,
    pub pitch: f32,
    pub yaw: f32,
    pub roll: f32,
    pub fire: bool,
    /// Held down on something usable.
    pub use_key: bool,
    /// Pressed to leave the landed ship or to board it again; one press is one
    /// change, however long it is held.
    pub board: bool,
}

fn unit(v: f32) -> f32 {
    if v.is_finite() {
        v.clamp(-1.0, 1.0)
    } else {
        0.0
    }
}

impl Input {
    /// Clamps every axis to `-1..=1`, maps NaN to 0 and limits the thrust
    /// vector to unit length so diagonals are no faster than straight lines.
    /// Devices and replays are untrusted, so the simulation always sees this.
    pub fn sanitized(self) -> Self {
        let mut thrust = Vec3::new(
            unit(self.thrust.x),
            unit(self.thrust.y),
            unit(self.thrust.z),
        );
        let len = thrust.length();
        if len > 1.0 {
            thrust = thrust * (1.0 / len);
        }
        Self {
            thrust,
            pitch: unit(self.pitch),
            yaw: unit(self.yaw),
            roll: unit(self.roll),
            fire: self.fire,
            use_key: self.use_key,
            board: self.board,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_clamps_and_rejects_nan() {
        let i = Input {
            thrust: Vec3::new(5.0, f32::NAN, -9.0),
            pitch: f32::INFINITY,
            yaw: -3.0,
            roll: f32::NAN,
            fire: true,
            use_key: false,
            board: false,
        }
        .sanitized();
        assert!(i.thrust.length() <= 1.0 + 1e-6);
        assert_eq!(i.thrust.y, 0.0);
        assert_eq!((i.pitch, i.yaw, i.roll), (0.0, -1.0, 0.0));
        assert!(i.fire);
    }
}
