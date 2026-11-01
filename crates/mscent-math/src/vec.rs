//! Small fixed-size vectors. `Vec3` is `f32` for runtime data; `Vec3d` is the
//! `f64` twin used while building CSG.

use core::ops::{Add, Mul, Neg, Sub};

use crate::epsilon;

/// 2D `f32` vector (texture coordinates, screen positions).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Vec2 {
    pub x: f32,
    pub y: f32,
}

impl Vec2 {
    #[inline]
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }

    #[inline]
    pub fn dot(self, o: Self) -> f32 {
        self.x * o.x + self.y * o.y
    }
}

impl Add for Vec2 {
    type Output = Self;
    #[inline]
    fn add(self, o: Self) -> Self {
        Self::new(self.x + o.x, self.y + o.y)
    }
}

impl Sub for Vec2 {
    type Output = Self;
    #[inline]
    fn sub(self, o: Self) -> Self {
        Self::new(self.x - o.x, self.y - o.y)
    }
}

impl Mul<f32> for Vec2 {
    type Output = Self;
    #[inline]
    fn mul(self, s: f32) -> Self {
        Self::new(self.x * s, self.y * s)
    }
}

/// 4D `f32` vector (homogeneous coordinates, RGBA).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Vec4 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub w: f32,
}

impl Vec4 {
    #[inline]
    pub const fn new(x: f32, y: f32, z: f32, w: f32) -> Self {
        Self { x, y, z, w }
    }

    #[inline]
    pub fn from_vec3(v: Vec3, w: f32) -> Self {
        Self::new(v.x, v.y, v.z, w)
    }

    #[inline]
    pub fn xyz(self) -> Vec3 {
        Vec3::new(self.x, self.y, self.z)
    }

    #[inline]
    pub fn dot(self, o: Self) -> f32 {
        self.x * o.x + self.y * o.y + self.z * o.z + self.w * o.w
    }
}

/// Generates a 3D vector type for one float width, so `Vec3` and `Vec3d`
/// cannot drift apart.
macro_rules! vec3_type {
    ($(#[$meta:meta])* $name:ident, $t:ty, $min_len:expr) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, Default, PartialEq)]
        pub struct $name {
            pub x: $t,
            pub y: $t,
            pub z: $t,
        }

        impl $name {
            pub const ZERO: Self = Self::new(0.0, 0.0, 0.0);

            #[inline]
            pub const fn new(x: $t, y: $t, z: $t) -> Self {
                Self { x, y, z }
            }

            #[inline]
            pub fn dot(self, o: Self) -> $t {
                self.x * o.x + self.y * o.y + self.z * o.z
            }

            #[inline]
            pub fn cross(self, o: Self) -> Self {
                Self::new(
                    self.y * o.z - self.z * o.y,
                    self.z * o.x - self.x * o.z,
                    self.x * o.y - self.y * o.x,
                )
            }

            #[inline]
            pub fn length_squared(self) -> $t {
                self.dot(self)
            }

            #[inline]
            pub fn length(self) -> $t {
                self.length_squared().sqrt()
            }

            /// Unit vector, or `None` when the length is too small to
            /// normalize safely, or not finite.
            #[inline]
            pub fn normalized(self) -> Option<Self> {
                let len = self.length();
                if len.is_finite() && len > $min_len {
                    Some(self * (1.0 / len))
                } else {
                    None
                }
            }

            #[inline]
            pub fn lerp(self, o: Self, t: $t) -> Self {
                self + (o - self) * t
            }

            #[inline]
            pub fn is_finite(self) -> bool {
                self.x.is_finite() && self.y.is_finite() && self.z.is_finite()
            }

            /// Component-wise minimum.
            #[inline]
            pub fn min(self, o: Self) -> Self {
                Self::new(self.x.min(o.x), self.y.min(o.y), self.z.min(o.z))
            }

            /// Component-wise maximum.
            #[inline]
            pub fn max(self, o: Self) -> Self {
                Self::new(self.x.max(o.x), self.y.max(o.y), self.z.max(o.z))
            }
        }

        impl Add for $name {
            type Output = Self;
            #[inline]
            fn add(self, o: Self) -> Self {
                Self::new(self.x + o.x, self.y + o.y, self.z + o.z)
            }
        }

        impl Sub for $name {
            type Output = Self;
            #[inline]
            fn sub(self, o: Self) -> Self {
                Self::new(self.x - o.x, self.y - o.y, self.z - o.z)
            }
        }

        impl Neg for $name {
            type Output = Self;
            #[inline]
            fn neg(self) -> Self {
                Self::new(-self.x, -self.y, -self.z)
            }
        }

        impl Mul<$t> for $name {
            type Output = Self;
            #[inline]
            fn mul(self, s: $t) -> Self {
                Self::new(self.x * s, self.y * s, self.z * s)
            }
        }
    };
}

vec3_type!(
    /// 3D `f32` vector for runtime data.
    Vec3,
    f32,
    epsilon::MIN_LENGTH_F32
);
vec3_type!(
    /// 3D `f64` vector used while building CSG and BSP data.
    Vec3d,
    f64,
    epsilon::MIN_LENGTH_F64
);

impl Vec3d {
    /// Rounds every component to the nearest multiple of `grid`. Non-finite
    /// input stays non-finite; callers validate with [`Vec3d::is_finite`].
    pub fn snapped(self, grid: f64) -> Self {
        let s = |v: f64| (v / grid).round() * grid;
        Self::new(s(self.x), s(self.y), s(self.z))
    }

    /// Narrows to the runtime `f32` type.
    pub fn to_f32(self) -> Vec3 {
        Vec3::new(self.x as f32, self.y as f32, self.z as f32)
    }
}

impl Vec3 {
    /// Widens to the build-time `f64` type.
    pub fn to_f64(self) -> Vec3d {
        Vec3d::new(f64::from(self.x), f64::from(self.y), f64::from(self.z))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cross_and_dot() {
        let (x, y) = (Vec3::new(1.0, 0.0, 0.0), Vec3::new(0.0, 1.0, 0.0));
        assert_eq!(x.cross(y), Vec3::new(0.0, 0.0, 1.0));
        assert_eq!(x.dot(y), 0.0);
    }

    #[test]
    fn normalize_rejects_degenerate() {
        assert!(Vec3::ZERO.normalized().is_none());
        assert!(Vec3d::new(f64::NAN, 0.0, 0.0).normalized().is_none());
        assert!(Vec3d::new(f64::INFINITY, 0.0, 0.0).normalized().is_none());
        let n = Vec3d::new(3.0, 0.0, 4.0).normalized().unwrap();
        assert!((n.length() - 1.0).abs() < 1e-12);
    }

    #[test]
    fn snapping_is_idempotent() {
        let v = Vec3d::new(0.3, -0.07, 12.49).snapped(epsilon::SNAP_GRID);
        assert_eq!(v, v.snapped(epsilon::SNAP_GRID));
        assert_eq!(v.x, 0.25);
        assert_eq!(v.y, -0.125);
        assert_eq!(v.z, 12.5);
    }

    #[test]
    fn widen_narrow_roundtrip() {
        let v = Vec3::new(1.5, -2.0, 0.25);
        assert_eq!(v.to_f64().to_f32(), v);
    }
}
