//! The 6DoF viewpoint: a position and a unit quaternion, so roll works.

use core::f32::consts::FRAC_PI_2;
use mscent_math::{Mat4, Quat, Vec3};

use crate::camera::Camera;

/// Where the player looks from. `orientation` rotates the camera frame
/// (right +X, up +Y, looking down -Z) into world space.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct View {
    pub pos: Vec3,
    pub orientation: Quat,
}

impl View {
    /// A viewpoint in the Z-up world from Euler-style angles, in radians:
    /// `yaw` turns left about +Z (0 looks along +Y), `pitch` looks up and
    /// `roll` tilts about the view axis. Use `orientation` directly for
    /// anything that must not suffer from Euler limits.
    pub fn from_angles(pos: Vec3, yaw: f32, pitch: f32, roll: f32) -> Self {
        let axis_x = Vec3::new(1.0, 0.0, 0.0);
        let axis_z = Vec3::new(0.0, 0.0, 1.0);
        // Camera up becomes world +Z and camera forward world +Y.
        let upright = Quat::from_axis_angle(axis_x, FRAC_PI_2);
        let orientation = Quat::from_axis_angle(axis_z, yaw)
            * upright
            * Quat::from_axis_angle(axis_x, pitch)
            * Quat::from_axis_angle(axis_z, roll);
        Self {
            pos,
            orientation: orientation.normalized(),
        }
    }

    /// Unit vector the camera looks along.
    pub fn forward(&self) -> Vec3 {
        self.orientation.rotate(Vec3::new(0.0, 0.0, -1.0))
    }

    /// The rasterizer camera for a `width` x `height` target. `None` when the
    /// pose or the size is invalid.
    pub fn camera(
        &self,
        fov_y: f32,
        near: f32,
        far: f32,
        width: u32,
        height: u32,
    ) -> Option<Camera> {
        if !self.pos.is_finite() {
            return None;
        }
        let to_world = self.orientation.to_mat3();
        let rot = to_world.transpose();
        let t = -rot.mul_vec(self.pos);
        let view = Mat4::from_rotation_translation(rot, t);
        Camera::from_view(view, fov_y, near, far, width, height)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::eye_of;

    fn close(a: Vec3, b: Vec3) -> bool {
        (a - b).length() < 1e-4
    }

    #[test]
    fn yaw_zero_looks_along_plus_y_and_positive_yaw_turns_left() {
        let v = View::from_angles(Vec3::ZERO, 0.0, 0.0, 0.0);
        assert!(close(v.forward(), Vec3::new(0.0, 1.0, 0.0)));
        let left = View::from_angles(Vec3::ZERO, FRAC_PI_2, 0.0, 0.0);
        assert!(close(left.forward(), Vec3::new(-1.0, 0.0, 0.0)));
        let up = View::from_angles(Vec3::ZERO, 0.0, FRAC_PI_2, 0.0);
        assert!(close(up.forward(), Vec3::new(0.0, 0.0, 1.0)));
    }

    #[test]
    fn camera_keeps_the_eye_and_projects_the_look_target_to_the_centre() {
        let eye = Vec3::new(3.0, -2.0, 5.0);
        let v = View::from_angles(eye, 0.7, 0.2, 0.5);
        let cam = v.camera(1.2, 0.1, 100.0, 64, 48).expect("camera");
        assert!(close(eye_of(&cam), eye));
        let p = cam
            .project(cam.world_to_camera(eye + v.forward() * 10.0))
            .expect("in front");
        assert!(
            (p.x - 32.0).abs() < 0.01 && (p.y - 24.0).abs() < 0.01,
            "{p:?}"
        );
    }

    #[test]
    fn the_eye_is_recovered_far_from_the_origin() {
        // A vista's room sits far out. Below the floor here, the floor's faces
        // were culled as back faces and the stars showed through the ground.
        let eye = Vec3::new(16384.0, 12.0, 0.75);
        let v = View::from_angles(eye, -0.8, 0.1, 0.3);
        let cam = v.camera(1.2, 0.1, 100.0, 64, 48).expect("camera");
        let got = eye_of(&cam);
        assert!((got - eye).length() < 0.02, "{got:?}");
    }

    #[test]
    fn roll_tilts_the_world_horizon() {
        let eye = Vec3::ZERO;
        let level = View::from_angles(eye, 0.0, 0.0, 0.0)
            .camera(1.2, 0.1, 100.0, 64, 64)
            .unwrap();
        let rolled = View::from_angles(eye, 0.0, 0.0, 0.5)
            .camera(1.2, 0.1, 100.0, 64, 64)
            .unwrap();
        let above = Vec3::new(0.0, 10.0, 3.0);
        let a = level.project(level.world_to_camera(above)).unwrap();
        let b = rolled.project(rolled.world_to_camera(above)).unwrap();
        assert!(
            (a.x - b.x).abs() > 2.0,
            "roll must move a point sideways: {a:?} {b:?}"
        );
    }
}
