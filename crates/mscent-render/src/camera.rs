//! Camera and perspective projection.
//!
//! Camera space is right-handed with +X right, +Y up and the view direction
//! along -Z (the convention of `Mat4::look_at`). Screen space has the origin
//! at the top-left, +X right and +Y down, with pixel centres at half
//! integers.

use mscent_math::{Mat4, Vec3};

/// Largest accepted framebuffer side.
pub const MAX_SIDE: u32 = 8192;

/// A projected camera-space point.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScreenPoint {
    pub x: f32,
    pub y: f32,
    /// Reciprocal of the view depth; linear in screen space, so it is what
    /// the z-buffer stores.
    pub inv_z: f32,
}

/// Pinhole camera for a fixed render target size.
#[derive(Clone, Debug)]
pub struct Camera {
    view: Mat4,
    fov_y: f32,
    near: f32,
    far: f32,
    width: u32,
    height: u32,
}

impl Camera {
    /// Camera at `eye` looking at `target`. `None` for degenerate vectors, a
    /// bad field of view or clip range, or a target size outside 1..=8192.
    #[allow(clippy::too_many_arguments)]
    pub fn look_at(
        eye: Vec3,
        target: Vec3,
        up: Vec3,
        fov_y: f32,
        near: f32,
        far: f32,
        width: u32,
        height: u32,
    ) -> Option<Self> {
        let view = Mat4::look_at(eye, target, up)?;
        Self::from_view(view, fov_y, near, far, width, height)
    }

    /// Camera from a world-to-camera matrix (for example an inverted entity
    /// transform).
    pub fn from_view(
        view: Mat4,
        fov_y: f32,
        near: f32,
        far: f32,
        width: u32,
        height: u32,
    ) -> Option<Self> {
        // Reuse the matrix validation so both paths reject the same inputs.
        Mat4::perspective(fov_y, width as f32 / height.max(1) as f32, near, far)?;
        let side_ok = |s: u32| (1..=MAX_SIDE).contains(&s);
        if !side_ok(width) || !side_ok(height) {
            return None;
        }
        Some(Self {
            view,
            fov_y,
            near,
            far,
            width,
            height,
        })
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    pub fn near(&self) -> f32 {
        self.near
    }

    pub fn far(&self) -> f32 {
        self.far
    }

    /// Focal length in pixels (square pixels).
    pub fn focal(&self) -> f32 {
        self.height as f32 * 0.5 / (self.fov_y * 0.5).tan()
    }

    /// World point in camera space.
    pub fn world_to_camera(&self, p: Vec3) -> Vec3 {
        self.view.transform_point(p).xyz()
    }

    /// Project a camera-space point. `None` at or behind the eye plane.
    pub fn project(&self, p: Vec3) -> Option<ScreenPoint> {
        let depth = -p.z;
        if depth <= 0.0 || !depth.is_finite() {
            return None;
        }
        let inv_z = 1.0 / depth;
        let f = self.focal();
        Some(ScreenPoint {
            x: self.width as f32 * 0.5 + p.x * inv_z * f,
            y: self.height as f32 * 0.5 - p.y * inv_z * f,
            inv_z,
        })
    }

    /// The OpenGL-style clip-space projection matching [`Camera::project`].
    pub fn projection_matrix(&self) -> Mat4 {
        let aspect = self.width as f32 / self.height as f32;
        // Construction validated the parameters, so the fallback is unreachable.
        Mat4::perspective(self.fov_y, aspect, self.near, self.far).unwrap_or(Mat4::IDENTITY)
    }

    /// Frustum planes in camera space.
    pub fn frustum(&self) -> Frustum {
        let ty = (self.fov_y * 0.5).tan();
        let tx = ty * self.width as f32 / self.height as f32;
        Frustum::new(tx, ty, self.near, self.far)
    }
}

/// Six inward-facing planes `n . p + k >= 0` in camera space.
#[derive(Clone, Copy, Debug)]
pub struct Frustum {
    planes: [(Vec3, f32); 6],
}

impl Frustum {
    /// `tx`/`ty` are the half-extents of the view at unit depth.
    pub fn new(tx: f32, ty: f32, near: f32, far: f32) -> Self {
        Self {
            planes: [
                (Vec3::new(0.0, 0.0, -1.0), -near),
                (Vec3::new(1.0, 0.0, -tx), 0.0),
                (Vec3::new(-1.0, 0.0, -tx), 0.0),
                (Vec3::new(0.0, 1.0, -ty), 0.0),
                (Vec3::new(0.0, -1.0, -ty), 0.0),
                (Vec3::new(0.0, 0.0, 1.0), far),
            ],
        }
    }

    pub fn planes(&self) -> &[(Vec3, f32); 6] {
        &self.planes
    }

    /// Signed distance of `p` to plane `i`; positive is inside.
    pub fn distance(&self, i: usize, p: Vec3) -> f32 {
        let (n, k) = self.planes[i];
        n.dot(p) + k
    }

    pub fn contains(&self, p: Vec3) -> bool {
        (0..6).all(|i| self.distance(i, p) >= 0.0)
    }
}

#[cfg(test)]
#[path = "camera_tests.rs"]
mod camera_tests;
