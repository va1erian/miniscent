//! Face-aware texture operations: fit, justify, align and edge-continuing
//! copy. All take the face polygon (`f64`, as extracted by `extract_faces`)
//! and do nothing, returning `false` or `None`, on degenerate input.

use super::{MAX_SCALE, MIN_SCALE, TexInfo, clamp_shift, unit};
use mscent_math::{Polygon, Vec3d};

/// Edge of the face's texture-space bounding box to align against.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Justify {
    /// Smallest u at texel 0.
    Left,
    /// Largest u at the texture's right edge.
    Right,
    /// Smallest v at texel 0.
    Top,
    /// Largest v at the texture's bottom edge.
    Bottom,
    /// Centre of the box at the centre of the texture, both axes.
    Center,
}

/// Distance under which two points count as one when matching edges.
const EDGE_EPS: f64 = 1.0e-4;

/// Texel range `(min, max)` of the polygon along (u, v), ignoring shift.
fn texel_bounds(info: &TexInfo, verts: &[Vec3d]) -> Option<[(f64, f64); 2]> {
    let (u, v) = info.effective_axes();
    let (su, sv) = (f64::from(info.u_scale), f64::from(info.v_scale));
    let mut b = [(f64::INFINITY, f64::NEG_INFINITY); 2];
    for p in verts {
        for (slot, t) in b.iter_mut().zip([p.dot(u) / su, p.dot(v) / sv]) {
            *slot = (slot.0.min(t), slot.1.max(t));
        }
    }
    let ok = b.iter().all(|&(lo, hi)| lo.is_finite() && hi.is_finite());
    ok.then_some(b)
}

fn size_f64(texture_size: (u32, u32)) -> (f64, f64) {
    (
        f64::from(texture_size.0.max(1)),
        f64::from(texture_size.1.max(1)),
    )
}

fn rodrigues(v: Vec3d, axis: Vec3d, angle: f64) -> Vec3d {
    let (s, c) = angle.sin_cos();
    v * c + axis.cross(v) * s + axis * (axis.dot(v) * (1.0 - c))
}

impl TexInfo {
    /// Scales and shifts so the face spans `tiles_u` by `tiles_v` repeats of
    /// a `texture_size` texture, starting at texel 0. Axes and rotation are
    /// kept; the scale signs (flips) are kept. `false` if the face has no
    /// extent along an axis, a tile count is not positive and finite, or the
    /// scale would leave the allowed range (nothing changes then).
    pub fn fit(
        &mut self,
        face: &Polygon,
        texture_size: (u32, u32),
        tiles_u: f32,
        tiles_v: f32,
    ) -> bool {
        let tiles_ok = |t: f32| t.is_finite() && t > 0.0;
        if !tiles_ok(tiles_u) || !tiles_ok(tiles_v) {
            return false;
        }
        let (u, v) = self.effective_axes();
        let extent = |axis: Vec3d| {
            let ts = face.vertices().iter().map(|p| p.dot(axis));
            let lo = ts.clone().fold(f64::INFINITY, f64::min);
            ts.fold(f64::NEG_INFINITY, f64::max) - lo
        };
        let (w, h) = size_f64(texture_size);
        let want_u = extent(u) / (w * f64::from(tiles_u));
        let want_v = extent(v) / (h * f64::from(tiles_v));
        let range = f64::from(MIN_SCALE)..=f64::from(MAX_SCALE);
        if !range.contains(&want_u) || !range.contains(&want_v) {
            return false;
        }
        self.u_scale = (want_u as f32).copysign(self.u_scale);
        self.v_scale = (want_v as f32).copysign(self.v_scale);
        self.u_shift = 0.0;
        self.v_shift = 0.0;
        let Some(b) = texel_bounds(self, face.vertices()) else {
            return false;
        };
        self.u_shift = clamp_shift(-b[0].0 as f32);
        self.v_shift = clamp_shift(-b[1].0 as f32);
        true
    }

    /// Shifts the texture so the face's texel box touches the chosen edge.
    /// The shift is kept in `0..texture size`, which looks identical because
    /// textures repeat. `false` on a degenerate face.
    pub fn justify(&mut self, face: &Polygon, texture_size: (u32, u32), how: Justify) -> bool {
        let Some(b) = texel_bounds(self, face.vertices()) else {
            return false;
        };
        let (w, h) = size_f64(texture_size);
        let [(umin, umax), (vmin, vmax)] = b;
        let (mut su, mut sv) = (f64::from(self.u_shift), f64::from(self.v_shift));
        match how {
            Justify::Left => su = -umin,
            Justify::Right => su = w - umax,
            Justify::Top => sv = -vmin,
            Justify::Bottom => sv = h - vmax,
            Justify::Center => {
                su = w / 2.0 - (umin + umax) / 2.0;
                sv = h / 2.0 - (vmin + vmax) / 2.0;
            }
        }
        self.u_shift = clamp_shift(su.rem_euclid(w) as f32);
        self.v_shift = clamp_shift(sv.rem_euclid(h) as f32);
        true
    }

    /// Hammer "align to world": the default axes for `normal`, no rotation.
    /// Scale, shift, material and lock are kept.
    pub fn align_to_world(&mut self, normal: Vec3d) {
        let fresh = Self::default_for_normal(normal);
        self.u_axis = fresh.u_axis;
        self.v_axis = fresh.v_axis;
        self.rotation_deg = 0.0;
    }

    /// Hammer "align to face": u runs along the face's longest edge (the
    /// longest roughly horizontal one on walls), v is `normal x u`, flipped
    /// on walls so it points up (+Z), as in the defaults. `normal` is the
    /// face's visible normal. `false` on a degenerate face.
    pub fn align_to_face(&mut self, face: &Polygon, normal: Vec3d) -> bool {
        let Some(n) = normal.normalized() else {
            return false;
        };
        let wall = n.z.abs() < 0.7;
        let verts = face.vertices();
        let edges = verts.iter().enumerate().filter_map(|(i, &a)| {
            let e = verts[(i + 1) % verts.len()] - a;
            let in_plane = e - n * e.dot(n);
            let flat = !wall || in_plane.z.abs() < 0.5 * in_plane.length();
            flat.then_some(in_plane)
        });
        let longest = edges.max_by(|a, b| a.length().total_cmp(&b.length()));
        let Some(mut u) = longest.and_then(Vec3d::normalized) else {
            return false;
        };
        let dominant = [u.x, u.y, u.z]
            .into_iter()
            .max_by(|a, b| a.abs().total_cmp(&b.abs()))
            .unwrap_or(1.0);
        if dominant < 0.0 {
            u = -u;
        }
        let mut v = n.cross(u);
        if wall && v.z < 0.0 {
            v = -v;
        }
        self.u_axis = u.to_f32();
        self.v_axis = v.to_f32();
        self.rotation_deg = 0.0;
        true
    }

    /// Carries this mapping from face `src` onto the adjacent face `dst` so
    /// the texture continues over their shared edge, as if `dst` were
    /// unfolded flat into `src`'s plane. Returns the new TexInfo for `dst`
    /// (material and scales of `self`, rotation folded into the axes), or
    /// `None` if the polygons share no edge or one is degenerate.
    pub fn copy_across_edge(&self, src: &Polygon, dst: &Polygon) -> Option<Self> {
        let (ns, nd) = (src.normal()?, dst.normal()?);
        let (a, b) = shared_edge(src, dst)?;
        let e = (b - a).normalized()?;
        // Rotation about the edge that takes dst's plane into src's plane.
        let angle = e.dot(nd.cross(ns)).atan2(nd.dot(ns));
        let (u, v) = self.effective_axes();
        // A point p of dst unfolds to a + R (p - a), so each axis maps to
        // R^-1 axis and the offset along the old axis moves into the shift.
        let (u2, v2) = (
            unit(rodrigues(u, e, -angle).to_f32())?,
            unit(rodrigues(v, e, -angle).to_f32())?,
        );
        let shift = |old: f32, scale: f32, axis: Vec3d, axis2: Vec3d| {
            let k = (axis.dot(a) - axis2.dot(a)) / f64::from(scale);
            clamp_shift(old + k as f32)
        };
        Some(Self {
            u_axis: u2,
            v_axis: v2,
            u_shift: shift(self.u_shift, self.u_scale, u, u2.to_f64()),
            v_shift: shift(self.v_shift, self.v_scale, v, v2.to_f64()),
            rotation_deg: 0.0,
            ..*self
        })
    }
}

/// An edge `(a, b)` of `src` that `dst` also has, in either direction.
fn shared_edge(src: &Polygon, dst: &Polygon) -> Option<(Vec3d, Vec3d)> {
    let near = |p: Vec3d, q: Vec3d| (p - q).length() <= EDGE_EPS;
    let (vs, vd) = (src.vertices(), dst.vertices());
    for (i, &a) in vs.iter().enumerate() {
        let b = vs[(i + 1) % vs.len()];
        for (j, &c) in vd.iter().enumerate() {
            let d = vd[(j + 1) % vd.len()];
            if (near(a, c) && near(b, d)) || (near(a, d) && near(b, c)) {
                return Some((a, b));
            }
        }
    }
    None
}
