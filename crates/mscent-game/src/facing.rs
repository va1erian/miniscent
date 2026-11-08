//! Which way an enemy points (M19.2), worked out from square roots alone.
//!
//! Facing turns an enemy's hit spheres, so it decides hits and is part of what a
//! replay must repeat exactly. IEEE rounds a square root the same on every
//! platform, but not a sine or an arc cosine, so neither is used here: the turn
//! toward a direction comes from half-angle formulas, and easing is a normalized
//! linear blend rather than [`Quat::slerp`].

use mscent_math::{Quat, Vec3};

/// Below this horizontal length a direction is straight up or down, and keeps
/// no yaw.
const MIN_FLAT: f32 = 1.0e-6;

/// The turn that points +X along `dir`, with no roll: a yaw about +Z after a
/// pitch about Y. `None` for a zero or non-finite direction.
pub(crate) fn toward(dir: Vec3) -> Option<Quat> {
    let d = dir.normalized()?;
    let flat = (d.x * d.x + d.y * d.y).sqrt();
    let yaw = if flat > MIN_FLAT {
        let (w, k) = half_angle(d.x / flat, d.y);
        Quat {
            x: 0.0,
            y: 0.0,
            z: k,
            w,
        }
    } else {
        Quat::IDENTITY
    };
    // Turning +X up by the pitch is a turn about +Y the other way.
    let (w, k) = half_angle(flat, d.z);
    let pitch = Quat {
        x: 0.0,
        y: -k,
        z: 0.0,
        w,
    };
    Some((yaw * pitch).normalized())
}

/// Cosine and sine of half an angle whose cosine is `c`, with the sign of `s`,
/// the angle's sine, for an angle within half a turn either way.
fn half_angle(c: f32, s: f32) -> (f32, f32) {
    let c = c.clamp(-1.0, 1.0);
    let cos = ((1.0 + c) * 0.5).sqrt();
    let sin = ((1.0 - c) * 0.5).sqrt();
    (cos, if s < 0.0 { -sin } else { sin })
}

/// `from` turned a fraction `t` of the way to `to`, the shorter way round.
pub(crate) fn ease(from: Quat, to: Quat, t: f32) -> Quat {
    let to = if from.dot(to) < 0.0 {
        Quat {
            x: -to.x,
            y: -to.y,
            z: -to.z,
            w: -to.w,
        }
    } else {
        to
    };
    let a = 1.0 - t;
    Quat {
        x: from.x * a + to.x * t,
        y: from.y * a + to.y * t,
        z: from.z * a + to.z * t,
        w: from.w * a + to.w * t,
    }
    .normalized()
}

#[cfg(test)]
mod tests {
    use super::*;

    const X: Vec3 = Vec3::new(1.0, 0.0, 0.0);

    fn close(a: Vec3, b: Vec3) -> bool {
        (a - b).length() < 1.0e-5
    }

    #[test]
    fn toward_points_x_along_the_direction() {
        let dirs = [
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(-1.0, 0.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
            Vec3::new(0.0, -1.0, 0.0),
            Vec3::new(0.0, 0.0, 1.0),
            Vec3::new(0.0, 0.0, -1.0),
            Vec3::new(-3.0, -2.0, 1.0),
            Vec3::new(0.2, -5.0, -4.0),
        ];
        for d in dirs {
            let q = toward(d).unwrap();
            let want = d.normalized().unwrap();
            assert!(close(q.rotate(X), want), "{d:?} gave {:?}", q.rotate(X));
        }
    }

    #[test]
    fn toward_keeps_the_top_up() {
        // No roll: the model's +Z stays in the vertical plane through its +X.
        let q = toward(Vec3::new(1.0, 1.0, 0.5)).unwrap();
        let up = q.rotate(Vec3::new(0.0, 0.0, 1.0));
        let side = q.rotate(Vec3::new(0.0, 1.0, 0.0));
        assert!(side.z.abs() < 1.0e-6, "rolled: {side:?}");
        assert!(up.z > 0.0);
    }

    #[test]
    fn nothing_to_face_is_none() {
        assert!(toward(Vec3::ZERO).is_none());
        assert!(toward(Vec3::new(f32::NAN, 0.0, 0.0)).is_none());
    }

    #[test]
    fn easing_arrives_and_takes_the_short_way() {
        let to = toward(Vec3::new(-1.0, 0.1, 0.0)).unwrap();
        let mut q = Quat::IDENTITY;
        for _ in 0..200 {
            q = ease(q, to, 0.08);
        }
        assert!(close(q.rotate(X), to.rotate(X)));
        let half = ease(
            Quat::IDENTITY,
            toward(Vec3::new(0.0, 1.0, 0.0)).unwrap(),
            0.5,
        );
        assert!(close(
            half.rotate(X),
            Vec3::new(1.0, 1.0, 0.0).normalized().unwrap()
        ));
    }
}
