//! Light on a voxel triangle. Colour and light direction belong to its vertices,
//! attached to world points and interpolated perspective-correctly, so a shadow
//! edge cannot slide with the span grid. The tangent frame of a plain triangle is
//! two world axes, so the bump maths of faces applies unchanged.
//!
//! The highlight is the one thing worked out per pixel, from the interpolated
//! normal and in world space. That frame is skewed wherever the surface tilts away
//! from its axis, so a highlight measured in it changes with the triangle's axis,
//! and draws the triangulation on anything glossy.

use mscent_math::Vec3;

use crate::bump::{FLAT_NORMAL, bump_gain, view_normal};
use crate::lighting_shader::{
    BUMP_FULL, BUMP_GONE, KNEE, MIN_SPEC, SPEC_GAIN, add_spec, dither_of, mul_rgb3, soft_limit,
};
use crate::shade::{Fog, SpanEnd, SpanShader, lerp_rgb};
use crate::specular::{SpecTable, half_vector};
use crate::texture::NormalView;

/// A vector that is linear in screen space: its value divided by depth.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Plane3 {
    pub at: Vec3,
    pub dx: Vec3,
    pub dy: Vec3,
}

impl Plane3 {
    /// The vector itself at the centre of pixel `(x, y)`, seen at `depth`.
    #[inline]
    pub(crate) fn value(&self, x: i32, y: i32, depth: f32) -> Vec3 {
        let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
        (self.at + self.dx * fx + self.dy * fy) * depth
    }
}

/// What a highlight is worked out from, as planes over the triangle. Empty when
/// a quality draws none.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Gloss {
    pub to_light: Plane3,
    pub pos: Plane3,
    pub normal: Plane3,
    pub eye: Vec3,
    pub u: Vec3,
    pub v: Vec3,
}

/// The half vector between the light and the eye, both from world points.
#[inline]
pub(super) fn world_half(to_light: Vec3, pos: Vec3, eye: Vec3) -> Vec3 {
    let l = to_light.normalized().unwrap_or(FLAT_NORMAL);
    half_vector(l, (eye - pos).normalized().unwrap_or(FLAT_NORMAL))
}

/// How much of `map`, a normal in the texture's frame, faces the half vector `h`,
/// on a surface facing `surface` and textured along `u` and `v`. Both vectors are
/// interpolated: this takes them back to unit length at about one multiply each.
#[inline]
pub(super) fn gloss_dot(map: Vec3, h: Vec3, surface: Vec3, u: Vec3, v: Vec3) -> f32 {
    let h = h * (1.5 - 0.5 * h.dot(h));
    let surface = surface * (1.5 - 0.5 * surface.dot(surface));
    map.x * h.dot(u) - map.y * h.dot(v) + map.z * h.dot(surface)
}

/// The light of one plain triangle.
pub(crate) struct VoxelShader<'a, const SPEC: bool> {
    pub light: Plane3,
    /// Light direction in the texture frame, when the bump needs it.
    pub dir: Plane3,
    pub gloss: Gloss,
    pub normals: Option<NormalView<'a>>,
    pub spec: Option<&'a SpecTable>,
    pub fog: Option<Fog>,
    pub col0: Vec3,
    pub dcol: Vec3,
    pub dir0: Vec3,
    pub ddir: Vec3,
    pub half0: Vec3,
    pub dhalf: Vec3,
    pub surface0: Vec3,
    pub dsurface: Vec3,
    pub bump_scale: f32,
    pub x0: i32,
    pub y0: i32,
    pub fog0: f32,
    pub fog_step: f32,
}

impl<const SPEC: bool> SpanShader for VoxelShader<'_, SPEC> {
    fn set_texel_rate(&mut self, texels_per_pixel: f32) {
        let t = (BUMP_GONE - texels_per_pixel) / (BUMP_GONE - BUMP_FULL);
        self.bump_scale = if t.is_nan() { 0.0 } else { t.clamp(0.0, 1.0) };
    }

    fn begin_span(&mut self, start: &SpanEnd, end: &SpanEnd, len: u32) {
        let inv = 1.0 / len.max(1) as f32;
        (self.x0, self.y0) = (start.x, start.y);
        let (a, b) = (
            self.light.value(start.x, start.y, start.depth),
            self.light.value(end.x, end.y, end.depth),
        );
        self.col0 = a;
        self.dcol = (b - a) * inv;
        if self.normals.is_some() && self.bump_scale > 0.0 || SPEC {
            let (a, b) = (
                self.dir.value(start.x, start.y, start.depth),
                self.dir.value(end.x, end.y, end.depth),
            );
            self.dir0 = a;
            self.ddir = (b - a) * inv;
        }
        if SPEC {
            let g = &self.gloss;
            let ends = [start, end].map(|e| {
                let at = |p: &Plane3| p.value(e.x, e.y, e.depth);
                let h = world_half(at(&g.to_light), at(&g.pos), g.eye);
                let n = at(&g.normal).normalized().unwrap_or(FLAT_NORMAL);
                (h, n)
            });
            self.half0 = ends[0].0;
            self.dhalf = (ends[1].0 - ends[0].0) * inv;
            self.surface0 = ends[0].1;
            self.dsurface = (ends[1].1 - ends[0].1) * inv;
        }
        if let Some(fog) = self.fog {
            self.fog0 = fog.amount(start.depth);
            self.fog_step = (fog.amount(end.depth) - self.fog0) * inv;
        }
    }

    #[inline]
    fn shade(&self, i: u32, texel: u32, fu: i32, fv: i32) -> u32 {
        let f = i as f32;
        let mut k = self.col0 + self.dcol * f;
        let mut n = FLAT_NORMAL;
        if let Some(map) = self.normals.filter(|_| self.bump_scale > 0.0) {
            n = view_normal(&map, fu, fv);
            if self.bump_scale < 1.0 {
                n = FLAT_NORMAL + (n - FLAT_NORMAL) * self.bump_scale;
            }
            k = k * bump_gain(n, self.dir0 + self.ddir * f);
        }
        if k.x.max(k.y).max(k.z) > KNEE {
            k = Vec3::new(soft_limit(k.x), soft_limit(k.y), soft_limit(k.z));
        }
        let mut out = mul_rgb3(texel, k, dither_of(self.x0 + i as i32, self.y0));
        if let Some(table) = self.spec.filter(|_| SPEC) {
            let (h, surface) = (
                self.half0 + self.dhalf * f,
                self.surface0 + self.dsurface * f,
            );
            let s = table.response(gloss_dot(n, h, surface, self.gloss.u, self.gloss.v));
            if s > MIN_SPEC {
                out = add_spec(out, k, s * SPEC_GAIN);
            }
        }
        match self.fog {
            Some(fog) => {
                let amount = (self.fog0 + self.fog_step * f).clamp(0.0, 256.0) as u32;
                if amount == 0 {
                    out
                } else {
                    lerp_rgb(out, fog.color, amount)
                }
            }
            None => out,
        }
    }
}
