//! Column-major `f32` matrices. Right-handed, +Y up, the camera looks down -Z
//! in view space, and clip-space depth runs from -w (near) to +w (far).

use core::ops::Mul;

use crate::vec::{Vec3, Vec4};

/// 3x3 matrix, stored as three columns. Used for rotations and bases.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Mat3 {
    pub cols: [Vec3; 3],
}

impl Mat3 {
    pub const IDENTITY: Self = Self::from_cols(
        Vec3::new(1.0, 0.0, 0.0),
        Vec3::new(0.0, 1.0, 0.0),
        Vec3::new(0.0, 0.0, 1.0),
    );

    pub const fn from_cols(x: Vec3, y: Vec3, z: Vec3) -> Self {
        Self { cols: [x, y, z] }
    }

    pub fn transpose(&self) -> Self {
        let [a, b, c] = self.cols;
        Self::from_cols(
            Vec3::new(a.x, b.x, c.x),
            Vec3::new(a.y, b.y, c.y),
            Vec3::new(a.z, b.z, c.z),
        )
    }

    /// Inverse of a pure rotation, which is its transpose. Only valid for
    /// orthonormal matrices; do not use it for scaled or sheared ones.
    pub fn inverse_rotation(&self) -> Self {
        self.transpose()
    }

    pub fn mul_vec(&self, v: Vec3) -> Vec3 {
        self.cols[0] * v.x + self.cols[1] * v.y + self.cols[2] * v.z
    }
}

impl Mul for Mat3 {
    type Output = Self;
    /// `(a * b).mul_vec(v) == a.mul_vec(b.mul_vec(v))`.
    fn mul(self, o: Self) -> Self {
        Self::from_cols(
            self.mul_vec(o.cols[0]),
            self.mul_vec(o.cols[1]),
            self.mul_vec(o.cols[2]),
        )
    }
}

/// 4x4 matrix, stored as four columns. Used for model, view and projection.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Mat4 {
    pub cols: [Vec4; 4],
}

impl Mat4 {
    pub const IDENTITY: Self = Self {
        cols: [
            Vec4::new(1.0, 0.0, 0.0, 0.0),
            Vec4::new(0.0, 1.0, 0.0, 0.0),
            Vec4::new(0.0, 0.0, 1.0, 0.0),
            Vec4::new(0.0, 0.0, 0.0, 1.0),
        ],
    };

    /// Rigid transform: rotate by `r`, then translate by `t`.
    pub fn from_rotation_translation(r: Mat3, t: Vec3) -> Self {
        Self {
            cols: [
                Vec4::from_vec3(r.cols[0], 0.0),
                Vec4::from_vec3(r.cols[1], 0.0),
                Vec4::from_vec3(r.cols[2], 0.0),
                Vec4::from_vec3(t, 1.0),
            ],
        }
    }

    /// The upper-left 3x3 block.
    pub fn rotation(&self) -> Mat3 {
        Mat3::from_cols(self.cols[0].xyz(), self.cols[1].xyz(), self.cols[2].xyz())
    }

    pub fn translation(&self) -> Vec3 {
        self.cols[3].xyz()
    }

    /// Inverse of a rotation-plus-translation matrix: `R^T` and `-R^T t`.
    /// Cheaper and more accurate than a general inverse; only valid when the
    /// rotation block is orthonormal (as camera and entity transforms are).
    pub fn inverse_rigid(&self) -> Self {
        let rt = self.rotation().inverse_rotation();
        Self::from_rotation_translation(rt, -rt.mul_vec(self.translation()))
    }

    pub fn mul_vec4(&self, v: Vec4) -> Vec4 {
        let [a, b, c, d] = self.cols;
        Vec4::new(
            a.x * v.x + b.x * v.y + c.x * v.z + d.x * v.w,
            a.y * v.x + b.y * v.y + c.y * v.z + d.y * v.w,
            a.z * v.x + b.z * v.y + c.z * v.z + d.z * v.w,
            a.w * v.x + b.w * v.y + c.w * v.z + d.w * v.w,
        )
    }

    /// Transforms a position (w = 1); the result is not divided by w.
    pub fn transform_point(&self, p: Vec3) -> Vec4 {
        self.mul_vec4(Vec4::from_vec3(p, 1.0))
    }

    /// Transforms a direction (w = 0), ignoring translation.
    pub fn transform_dir(&self, d: Vec3) -> Vec3 {
        self.mul_vec4(Vec4::from_vec3(d, 0.0)).xyz()
    }

    /// View matrix for a camera at `eye` looking at `target`. `None` when
    /// eye and target coincide or `up` is parallel to the view direction.
    pub fn look_at(eye: Vec3, target: Vec3, up: Vec3) -> Option<Self> {
        let forward = (target - eye).normalized()?;
        let right = forward.cross(up).normalized()?;
        let true_up = right.cross(forward);
        // Camera basis as rows of the world-to-view rotation: right, up, -forward.
        let rot = Mat3::from_cols(right, true_up, -forward).transpose();
        Some(Self::from_rotation_translation(rot, Vec3::ZERO).with_translation(-rot.mul_vec(eye)))
    }

    /// Perspective projection with a vertical field of view in radians.
    /// `None` for a non-positive aspect, near or far, or `far <= near`, or a
    /// field of view outside (0, pi).
    pub fn perspective(fov_y: f32, aspect: f32, near: f32, far: f32) -> Option<Self> {
        let valid = fov_y > 0.0
            && fov_y < core::f32::consts::PI
            && aspect > 0.0
            && near > 0.0
            && far > near
            && [fov_y, aspect, near, far].iter().all(|v| v.is_finite());
        if !valid {
            return None;
        }
        let f = 1.0 / (fov_y * 0.5).tan();
        let depth = far - near;
        Some(Self {
            cols: [
                Vec4::new(f / aspect, 0.0, 0.0, 0.0),
                Vec4::new(0.0, f, 0.0, 0.0),
                Vec4::new(0.0, 0.0, -(far + near) / depth, -1.0),
                Vec4::new(0.0, 0.0, -2.0 * far * near / depth, 0.0),
            ],
        })
    }

    fn with_translation(mut self, t: Vec3) -> Self {
        self.cols[3] = Vec4::from_vec3(t, 1.0);
        self
    }
}

impl Mul for Mat4 {
    type Output = Self;
    /// `(a * b).transform(v) == a.transform(b.transform(v))`.
    fn mul(self, o: Self) -> Self {
        Self {
            cols: o.cols.map(|c| self.mul_vec4(c)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: Vec3, b: Vec3) -> bool {
        (a - b).length() < 1e-5
    }

    fn rot_z_90() -> Mat3 {
        Mat3::from_cols(
            Vec3::new(0.0, 1.0, 0.0),
            Vec3::new(-1.0, 0.0, 0.0),
            Vec3::new(0.0, 0.0, 1.0),
        )
    }

    #[test]
    fn mat3_mul_composes_right_to_left() {
        let r = rot_z_90();
        let v = Vec3::new(1.0, 0.0, 0.0);
        assert!(close((r * r).mul_vec(v), Vec3::new(-1.0, 0.0, 0.0)));
        assert_eq!(Mat3::IDENTITY * r, r);
    }

    #[test]
    fn mat4_mul_composes_right_to_left() {
        let rot = Mat4::from_rotation_translation(rot_z_90(), Vec3::ZERO);
        let tr = Mat4::from_rotation_translation(Mat3::IDENTITY, Vec3::new(10.0, 0.0, 0.0));
        let p = Vec3::new(1.0, 0.0, 0.0);
        // Rotate first, then translate.
        assert!(close(
            (tr * rot).transform_point(p).xyz(),
            Vec3::new(10.0, 1.0, 0.0)
        ));
        assert!(close(
            (rot * tr).transform_point(p).xyz(),
            Vec3::new(0.0, 11.0, 0.0)
        ));
        assert_eq!(Mat4::IDENTITY * rot, rot);
    }

    #[test]
    fn inverse_rigid_undoes_transform() {
        let m = Mat4::from_rotation_translation(rot_z_90(), Vec3::new(3.0, -4.0, 5.0));
        let id = m * m.inverse_rigid();
        for (i, col) in id.cols.iter().enumerate() {
            for (j, v) in [col.x, col.y, col.z, col.w].iter().enumerate() {
                let want = if i == j { 1.0 } else { 0.0 };
                assert!((v - want).abs() < 1e-5, "[{i}][{j}] = {v}");
            }
        }
    }

    #[test]
    fn look_at_puts_eye_at_origin_and_target_on_minus_z() {
        let eye = Vec3::new(1.0, 2.0, 3.0);
        let target = Vec3::new(1.0, 2.0, -7.0);
        let v = Mat4::look_at(eye, target, Vec3::new(0.0, 1.0, 0.0)).unwrap();
        assert!(close(v.transform_point(eye).xyz(), Vec3::ZERO));
        assert!(close(
            v.transform_point(target).xyz(),
            Vec3::new(0.0, 0.0, -10.0)
        ));
        assert!(close(
            v.inverse_rigid().transform_point(Vec3::ZERO).xyz(),
            eye
        ));
    }

    #[test]
    fn look_at_rejects_degenerate() {
        let up = Vec3::new(0.0, 1.0, 0.0);
        let e = Vec3::ZERO;
        assert!(Mat4::look_at(e, e, up).is_none());
        assert!(Mat4::look_at(e, Vec3::new(0.0, 5.0, 0.0), up).is_none());
    }

    #[test]
    fn perspective_maps_near_and_far_to_clip_extremes() {
        let p = Mat4::perspective(1.0, 16.0 / 9.0, 0.1, 100.0).unwrap();
        let near = p.transform_point(Vec3::new(0.0, 0.0, -0.1));
        let far = p.transform_point(Vec3::new(0.0, 0.0, -100.0));
        assert!((near.z / near.w + 1.0).abs() < 1e-4);
        assert!((far.z / far.w - 1.0).abs() < 1e-4);
    }

    #[test]
    fn perspective_rejects_bad_parameters() {
        assert!(Mat4::perspective(1.0, 1.0, 1.0, 1.0).is_none());
        assert!(Mat4::perspective(1.0, 1.0, 0.0, 10.0).is_none());
        assert!(Mat4::perspective(0.0, 1.0, 0.1, 10.0).is_none());
        assert!(Mat4::perspective(1.0, f32::NAN, 0.1, 10.0).is_none());
        assert!(Mat4::perspective(4.0, 1.0, 0.1, 10.0).is_none());
    }
}
