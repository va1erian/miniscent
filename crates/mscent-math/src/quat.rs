//! Unit quaternions for six-degrees-of-freedom orientation. Quaternions avoid
//! the gimbal lock that Euler angles suffer at 90 degrees of pitch.

use core::ops::Mul;

use crate::epsilon;
use crate::mat::Mat3;
use crate::vec::Vec3;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Quat {
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub w: f32,
}

impl Quat {
    pub const IDENTITY: Self = Self {
        x: 0.0,
        y: 0.0,
        z: 0.0,
        w: 1.0,
    };

    /// Rotation of `angle` radians about `axis` (right-handed). A zero or
    /// non-finite axis yields the identity rather than NaN.
    pub fn from_axis_angle(axis: Vec3, angle: f32) -> Self {
        let Some(a) = axis.normalized() else {
            return Self::IDENTITY;
        };
        let (s, c) = (angle * 0.5).sin_cos();
        Self {
            x: a.x * s,
            y: a.y * s,
            z: a.z * s,
            w: c,
        }
    }

    pub fn conjugate(self) -> Self {
        Self {
            x: -self.x,
            y: -self.y,
            z: -self.z,
            w: self.w,
        }
    }

    pub fn norm_squared(self) -> f32 {
        self.x * self.x + self.y * self.y + self.z * self.z + self.w * self.w
    }

    /// Renormalizes to unit length, to cancel drift after many `mul`s. A
    /// degenerate or non-finite quaternion becomes the identity.
    pub fn normalized(self) -> Self {
        let n = self.norm_squared().sqrt();
        if n.is_finite() && n > epsilon::MIN_LENGTH_F32 {
            let inv = 1.0 / n;
            Self {
                x: self.x * inv,
                y: self.y * inv,
                z: self.z * inv,
                w: self.w * inv,
            }
        } else {
            Self::IDENTITY
        }
    }

    /// Rotates a vector. Assumes `self` is unit length.
    pub fn rotate(self, v: Vec3) -> Vec3 {
        // v' = v + 2w(q x v) + 2 q x (q x v), the cheap form of q v q*.
        let q = Vec3::new(self.x, self.y, self.z);
        let t = q.cross(v) * 2.0;
        v + t * self.w + q.cross(t)
    }

    /// Rotation matrix equivalent to [`Quat::rotate`].
    pub fn to_mat3(self) -> Mat3 {
        Mat3::from_cols(
            self.rotate(Vec3::new(1.0, 0.0, 0.0)),
            self.rotate(Vec3::new(0.0, 1.0, 0.0)),
            self.rotate(Vec3::new(0.0, 0.0, 1.0)),
        )
    }
}

impl Mul for Quat {
    type Output = Self;
    /// Hamilton product; `(a * b).rotate(v) == a.rotate(b.rotate(v))`.
    fn mul(self, o: Self) -> Self {
        Self {
            w: self.w * o.w - self.x * o.x - self.y * o.y - self.z * o.z,
            x: self.w * o.x + self.x * o.w + self.y * o.z - self.z * o.y,
            y: self.w * o.y - self.x * o.z + self.y * o.w + self.z * o.x,
            z: self.w * o.z + self.x * o.y - self.y * o.x + self.z * o.w,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::f32::consts::FRAC_PI_2;

    fn close(a: Vec3, b: Vec3) -> bool {
        (a - b).length() < 1e-5
    }

    const X: Vec3 = Vec3::new(1.0, 0.0, 0.0);
    const Y: Vec3 = Vec3::new(0.0, 1.0, 0.0);
    const Z: Vec3 = Vec3::new(0.0, 0.0, 1.0);

    #[test]
    fn quarter_turn_about_z() {
        let q = Quat::from_axis_angle(Z, FRAC_PI_2);
        assert!(close(q.rotate(X), Y));
        assert!(close(q.rotate(Z), Z));
    }

    #[test]
    fn mul_applies_right_operand_first() {
        let about_x = Quat::from_axis_angle(X, FRAC_PI_2);
        let about_z = Quat::from_axis_angle(Z, FRAC_PI_2);
        let v = Y;
        assert!(close(
            (about_x * about_z).rotate(v),
            about_x.rotate(about_z.rotate(v))
        ));
        assert!(!close(
            (about_x * about_z).rotate(v),
            (about_z * about_x).rotate(v)
        ));
    }

    #[test]
    fn to_mat3_matches_rotate() {
        let q = Quat::from_axis_angle(Vec3::new(1.0, 2.0, 3.0), 1.1);
        let v = Vec3::new(-0.3, 0.8, 2.0);
        assert!(close(q.to_mat3().mul_vec(v), q.rotate(v)));
    }

    #[test]
    fn conjugate_inverts() {
        let q = Quat::from_axis_angle(Vec3::new(1.0, -1.0, 0.5), 2.0);
        let v = Vec3::new(1.0, 2.0, 3.0);
        assert!(close(q.conjugate().rotate(q.rotate(v)), v));
    }

    #[test]
    fn normalize_cancels_drift_and_handles_degenerate() {
        let mut q = Quat::from_axis_angle(Vec3::new(0.3, 0.9, 0.1), 0.01);
        let step = q;
        for _ in 0..10_000 {
            q = (q * step).normalized();
        }
        assert!((q.norm_squared() - 1.0).abs() < epsilon::UNIT_NORM_TOLERANCE);
        let zero = Quat {
            x: 0.0,
            y: 0.0,
            z: 0.0,
            w: 0.0,
        };
        assert_eq!(zero.normalized(), Quat::IDENTITY);
        assert_eq!(Quat::from_axis_angle(Vec3::ZERO, 1.0), Quat::IDENTITY);
    }

    #[test]
    fn no_gimbal_lock_at_ninety_degrees_pitch() {
        // Pitch straight up, then yaw about the body's own up axis. With Euler
        // angles the yaw would be lost here; the quaternion still turns the
        // view by exactly the commanded angle and stays orthonormal.
        let pitch = Quat::from_axis_angle(X, FRAC_PI_2);
        let q = (pitch * Quat::from_axis_angle(Y, 0.5)).normalized();
        let fwd = q.rotate(Vec3::new(0.0, 0.0, -1.0));
        let fwd_before = pitch.rotate(Vec3::new(0.0, 0.0, -1.0));
        assert!((fwd.dot(fwd_before) - 0.5_f32.cos()).abs() < 1e-5);
        let m = q.to_mat3();
        assert!(m.cols[0].dot(m.cols[1]).abs() < 1e-5);
        assert!((m.cols[2].length() - 1.0).abs() < 1e-5);
    }
}
