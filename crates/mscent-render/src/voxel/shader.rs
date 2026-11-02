//! Light on a voxel triangle. Colour and light direction belong to its vertices,
//! attached to world points and interpolated perspective-correctly, so a shadow
//! edge cannot slide with the span grid. The tangent frame of a plain triangle is
//! two world axes, so the bump and specular maths of faces applies unchanged.

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

/// The light of one plain triangle.
pub(crate) struct VoxelShader<'a, const SPEC: bool> {
    pub light: Plane3,
    /// Light direction in the texture frame, when the bump needs it.
    pub dir: Plane3,
    /// Direction to the eye in the texture frame, for specular.
    pub view: Plane3,
    pub normals: Option<NormalView<'a>>,
    pub spec: Option<&'a SpecTable>,
    pub fog: Option<Fog>,
    pub col0: Vec3,
    pub dcol: Vec3,
    pub dir0: Vec3,
    pub ddir: Vec3,
    pub half0: Vec3,
    pub dhalf: Vec3,
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
            if SPEC {
                let half = |d: Vec3, v: Vec3| {
                    let l = d.normalized().unwrap_or(FLAT_NORMAL);
                    half_vector(l, v.normalized().unwrap_or(FLAT_NORMAL))
                };
                let h0 = half(a, self.view.value(start.x, start.y, start.depth));
                let h1 = half(b, self.view.value(end.x, end.y, end.depth));
                self.half0 = h0;
                self.dhalf = (h1 - h0) * inv;
            }
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
            let h = self.half0 + self.dhalf * f;
            let s = table.response(n.dot(h) * (1.5 - 0.5 * h.dot(h)));
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
