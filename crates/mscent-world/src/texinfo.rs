//! Per-face texture mapping, Hammer style.
//!
//! # Model
//!
//! Every brush plane carries a [`TexInfo`]. A texture coordinate is a pure
//! function of the world position, so faces cut from one plane (by CSG or
//! merging) stay seamless, and it does not depend on which side is viewed.
//!
//! ```text
//! texel_u = (p . U) / u_scale + u_shift        texel_v = (p . V) / v_scale + v_shift
//! uv      = (texel_u / texture_width, texel_v / texture_height)   // texture repeats
//! ```
//!
//! `U` and `V` are the stored axes turned by `rotation_deg` inside their own
//! plane ([`TexInfo::effective_axes`]); positive rotation turns the image
//! counter-clockwise for an observer where `(U, V, normal)` is right-handed.
//!
//! * `u_scale` / `v_scale` are **world units per texel**, as Hammer's "texture
//!   scale": `0.25` is four texels per unit. The size of one texture tile in
//!   the world is `texture_size * scale`. A negative scale mirrors the image;
//!   magnitudes stay within [`MIN_SCALE`]..=[`MAX_SCALE`].
//! * `u_shift` / `v_shift` are in texels, within +-[`MAX_SHIFT`].
//! * [`DEFAULT_SCALE`] is `0.0625`: a 128 texel texture then repeats every
//!   8 world units, which is exactly what the renderer did before TexInfo.
//! * [`TexInfo::default_for_normal`] picks axes like Hammer's "align to
//!   world": floors and ceilings project onto X/Y, walls onto X/Z or Y/Z by
//!   the dominant normal axis. As before TexInfo, `V` is +Z on walls and the
//!   projection ignores the sign of the normal.
//! * `lock_to_world` is editor policy only, the renderer ignores it. When
//!   `false`, [`TexInfo::follow_translation`] compensates the shift so the
//!   texture moves with a translated brush; when `true` it stays put.
//!
//! # Operations
//!
//! Plain: [`rotate`](TexInfo::rotate), [`shift_by`](TexInfo::shift_by),
//! [`scale_by`](TexInfo::scale_by), [`set_scale`](TexInfo::set_scale),
//! [`flip_u`](TexInfo::flip_u), [`flip_v`](TexInfo::flip_v). Face-aware (in
//! `texinfo/align.rs`): `fit`, `justify`, `align_to_face`, `align_to_world`
//! and `copy_across_edge`, which continues a texture over a shared edge
//! (Hammer's apply-with-alignment).
//!
//! # Validation
//!
//! Operations ignore non-finite arguments and clamp their results.
//! [`TexInfo::sanitized`] repairs any value (used when an editor hands one
//! to a brush), while [`TexInfo::is_valid`] is the strict check the file
//! reader uses to reject hostile data.

mod align;

pub use align::Justify;

use mscent_math::{Vec3, Vec3d};

/// Smallest allowed scale magnitude, world units per texel.
pub const MIN_SCALE: f32 = 0.001;
/// Largest allowed scale magnitude.
pub const MAX_SCALE: f32 = 1000.0;
/// Largest allowed shift magnitude, in texels.
pub const MAX_SHIFT: f32 = 65_536.0;
/// Scale that makes a 128 texel texture repeat every 8 world units.
pub const DEFAULT_SCALE: f32 = 0.0625;
/// Axes shorter or longer than this (after the file's `f32` rounding) are
/// not unit vectors.
const AXIS_TOLERANCE: f32 = 0.01;
/// Axes whose cross product is shorter than this are treated as parallel.
const MIN_INDEPENDENCE: f64 = 1.0e-3;

/// How one brush plane maps its texture. See the module docs.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TexInfo {
    pub material: u16,
    pub u_axis: Vec3,
    pub v_axis: Vec3,
    pub u_shift: f32,
    pub v_shift: f32,
    pub u_scale: f32,
    pub v_scale: f32,
    pub rotation_deg: f32,
    pub lock_to_world: bool,
}

/// Wraps degrees into `[-180, 180)`.
fn wrap_degrees(d: f32) -> f32 {
    (d + 180.0).rem_euclid(360.0) - 180.0
}

/// Clamps a scale's magnitude, keeping its sign; junk becomes the default.
fn clamp_scale(s: f32) -> f32 {
    if !s.is_finite() || s == 0.0 {
        return DEFAULT_SCALE;
    }
    s.signum() * s.abs().clamp(MIN_SCALE, MAX_SCALE)
}

fn clamp_shift(s: f32) -> f32 {
    if s.is_finite() {
        s.clamp(-MAX_SHIFT, MAX_SHIFT)
    } else {
        0.0
    }
}

/// Unit vector or `None`.
fn unit(v: Vec3) -> Option<Vec3> {
    v.to_f64().normalized().map(Vec3d::to_f32)
}

fn independent(u: Vec3, v: Vec3) -> bool {
    u.to_f64().cross(v.to_f64()).length() >= MIN_INDEPENDENCE
}

impl TexInfo {
    /// Default mapping for a face with this normal (Hammer "align to
    /// world"), material 0. A zero or non-finite normal projects like a floor.
    pub fn default_for_normal(normal: Vec3d) -> Self {
        let (axis_u, axis_v) = world_axes(normal);
        Self {
            material: crate::brush::DEFAULT_MATERIAL,
            u_axis: axis_u,
            v_axis: axis_v,
            u_shift: 0.0,
            v_shift: 0.0,
            u_scale: DEFAULT_SCALE,
            v_scale: DEFAULT_SCALE,
            rotation_deg: 0.0,
            lock_to_world: true,
        }
    }

    /// Axes after applying `rotation_deg`, as `f64` unit-ish vectors.
    pub fn effective_axes(&self) -> (Vec3d, Vec3d) {
        let (u, v) = (self.u_axis.to_f64(), self.v_axis.to_f64());
        let (s, c) = f64::from(self.rotation_deg).to_radians().sin_cos();
        (u * c + v * s, v * c - u * s)
    }

    /// Texel coordinates of `p` (not yet divided by the texture size).
    pub fn texels_at(&self, p: Vec3d) -> (f64, f64) {
        let (u, v) = self.effective_axes();
        (
            p.dot(u) / f64::from(self.u_scale) + f64::from(self.u_shift),
            p.dot(v) / f64::from(self.v_scale) + f64::from(self.v_shift),
        )
    }

    /// Texture coordinates of `p` in repeats for a texture of
    /// `texture_size = (width, height)` texels (zero counts as one). A
    /// non-finite result becomes `(0, 0)`.
    pub fn uv_at(&self, p: Vec3d, texture_size: (u32, u32)) -> (f32, f32) {
        let (tu, tv) = self.texels_at(p);
        let (w, h) = (
            f64::from(texture_size.0.max(1)),
            f64::from(texture_size.1.max(1)),
        );
        let (u, v) = ((tu / w) as f32, (tv / h) as f32);
        if u.is_finite() && v.is_finite() {
            (u, v)
        } else {
            (0.0, 0.0)
        }
    }

    /// Strict validity: everything finite and inside its documented range,
    /// axes unit length and independent.
    pub fn is_valid(&self) -> bool {
        let unit_len = |a: Vec3| (a.to_f64().length() - 1.0).abs() <= f64::from(AXIS_TOLERANCE);
        let scale_ok = |s: f32| s.is_finite() && (MIN_SCALE..=MAX_SCALE).contains(&s.abs());
        let shift_ok = |s: f32| s.is_finite() && s.abs() <= MAX_SHIFT;
        self.u_axis.is_finite()
            && self.v_axis.is_finite()
            && unit_len(self.u_axis)
            && unit_len(self.v_axis)
            && independent(self.u_axis, self.v_axis)
            && scale_ok(self.u_scale)
            && scale_ok(self.v_scale)
            && shift_ok(self.u_shift)
            && shift_ok(self.v_shift)
            && self.rotation_deg.is_finite()
            && self.rotation_deg.abs() <= 360.0
    }

    /// Repairs any value: axes are normalised (or replaced by the defaults
    /// for `normal` when degenerate), scales and shifts clamped, junk reset.
    pub fn sanitized(self, normal: Vec3d) -> Self {
        let fallback = world_axes(normal);
        let axes = match (unit(self.u_axis), unit(self.v_axis)) {
            (Some(u), Some(v)) if independent(u, v) => (u, v),
            _ => fallback,
        };
        Self {
            u_axis: axes.0,
            v_axis: axes.1,
            u_scale: clamp_scale(self.u_scale),
            v_scale: clamp_scale(self.v_scale),
            u_shift: clamp_shift(self.u_shift),
            v_shift: clamp_shift(self.v_shift),
            rotation_deg: if self.rotation_deg.is_finite() {
                wrap_degrees(self.rotation_deg)
            } else {
                0.0
            },
            ..self
        }
    }

    /// Turns the texture by `degrees` (counter-clockwise when positive).
    pub fn rotate(&mut self, degrees: f32) {
        if degrees.is_finite() {
            self.rotation_deg = wrap_degrees(self.rotation_deg + degrees);
        }
    }

    /// Slides the texture by `(du, dv)` texels.
    pub fn shift_by(&mut self, du: f32, dv: f32) {
        if du.is_finite() && dv.is_finite() {
            self.u_shift = clamp_shift(self.u_shift + du);
            self.v_shift = clamp_shift(self.v_shift + dv);
        }
    }

    /// Multiplies the scales; a zero or non-finite factor is ignored.
    pub fn scale_by(&mut self, fu: f32, fv: f32) {
        if fu.is_finite() && fv.is_finite() && fu != 0.0 && fv != 0.0 {
            self.u_scale = clamp_scale(self.u_scale * fu);
            self.v_scale = clamp_scale(self.v_scale * fv);
        }
    }

    /// Sets the scales (world units per texel); invalid values are repaired
    /// like in [`TexInfo::sanitized`].
    pub fn set_scale(&mut self, u_scale: f32, v_scale: f32) {
        self.u_scale = clamp_scale(u_scale);
        self.v_scale = clamp_scale(v_scale);
    }

    /// Mirrors the image along u about texel 0.
    pub fn flip_u(&mut self) {
        self.u_scale = -self.u_scale;
        self.u_shift = clamp_shift(-self.u_shift);
    }

    /// Mirrors the image along v about texel 0.
    pub fn flip_v(&mut self) {
        self.v_scale = -self.v_scale;
        self.v_shift = clamp_shift(-self.v_shift);
    }

    /// Adjusts the shift for a brush translated by `offset`. With
    /// `lock_to_world` the texture stays where it is in the world, otherwise
    /// it travels with the brush. Non-finite offsets are ignored.
    pub fn follow_translation(&mut self, offset: Vec3d) {
        if self.lock_to_world || !offset.is_finite() {
            return;
        }
        let (u, v) = self.effective_axes();
        let du = offset.dot(u) / f64::from(self.u_scale);
        let dv = offset.dot(v) / f64::from(self.v_scale);
        self.shift_by(-(du as f32), -(dv as f32));
    }
}

/// Hammer "align to world" axes for a normal.
fn world_axes(normal: Vec3d) -> (Vec3, Vec3) {
    let (ax, ay, az) = (normal.x.abs(), normal.y.abs(), normal.z.abs());
    let (x, y, z) = (
        Vec3::new(1.0, 0.0, 0.0),
        Vec3::new(0.0, 1.0, 0.0),
        Vec3::new(0.0, 0.0, 1.0),
    );
    // NaN compares false everywhere, which falls through to the floor case.
    if ax > az && ax >= ay {
        (y, z)
    } else if ay > az && ay > ax {
        (x, z)
    } else {
        (x, y)
    }
}

#[cfg(test)]
mod texinfo_tests;
