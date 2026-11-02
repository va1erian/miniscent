//! Triangles whose corners disagree on texture axis or material. Each pixel picks
//! one corner's look through a 4x4 ordered dither fixed on the screen: stable,
//! cheap, and only these triangles pay. The one mechanism serves both cases.
//!
//! They shade as their neighbours do, or they would show as patches on anything
//! glossy. With no one tangent frame, the light and the half vector are stepped in
//! world space and moved into the picked corner's frame per pixel. It costs a few
//! dot products more on these triangles alone.

use mscent_math::Vec3;

use super::fill::FillContext;
use super::tri::{Corner, Gradient, each_row};
use super::{TEXELS_PER_UNIT, axis_map};
use crate::bump::{FLAT_NORMAL, bump_gain, view_normal};
use crate::framebuffer::Framebuffer;
use crate::lighting_shader::{
    BUMP_FULL, BUMP_GONE, KNEE, MIN_SPEC, SPEC_GAIN, add_spec, dither_of, mul_rgb3, soft_limit,
};
use crate::raster::{DrawStats, SPAN_LEN};
use crate::shade::lerp_rgb;
use crate::specular::{SpecTable, half_vector};
use crate::texture::{LodSampler, NormalView, Texture};

/// Thresholds of the ordered dither, as fractions of 16.
const BAYER: [[u8; 4]; 4] = [[0, 8, 2, 10], [12, 4, 14, 6], [3, 11, 1, 9], [15, 7, 13, 5]];

/// One corner's look.
struct Look<'a> {
    u_axis: Vec3,
    v_axis: Vec3,
    lod: LodSampler<'a>,
    smooth: bool,
    normals: Option<NormalView<'a>>,
    spec: Option<&'a SpecTable>,
}

impl Look<'_> {
    /// Texel coordinates of `p` in 16.16 fixed point.
    #[inline]
    fn coords(&self, p: Vec3) -> (i32, i32) {
        let fixed = |v: f32| (f64::from(v) * f64::from(TEXELS_PER_UNIT) * 65536.0) as i64 as i32;
        (fixed(p.dot(self.u_axis)), fixed(p.dot(self.v_axis)))
    }

    #[inline]
    fn texel(&self, (fu, fv): (i32, i32)) -> u32 {
        if self.smooth {
            self.lod.bilinear(fu, fv)
        } else {
            self.lod.nearest(fu >> 16, fv >> 16)
        }
    }

    /// `v` in the frame the normal maps are authored in, on a surface facing `n`.
    #[inline]
    fn to_frame(&self, v: Vec3, n: Vec3) -> Vec3 {
        Vec3::new(v.dot(self.u_axis), -v.dot(self.v_axis), v.dot(n))
    }
}

/// Texels per pixel of a surface at `iz`, seen face on.
fn rate_of(cam_focal: f32, iz: f32) -> f32 {
    TEXELS_PER_UNIT / (cam_focal * iz.max(1e-6))
}

/// The mip level of a triangle: all corners share the texel density, so the
/// footprint of its depth is enough.
fn level_of(tex: &Texture, rate: f32) -> u32 {
    if tex.mip_count() == 0 || rate.is_nan() || rate <= 1.5 {
        return 0;
    }
    ((rate / 1.5).log2().ceil() as u32).min(tex.mip_count())
}

/// How much of the bump effect survives at `rate`, as for a plain triangle.
fn bump_scale(rate: f32) -> f32 {
    ((BUMP_GONE - rate) / (BUMP_GONE - BUMP_FULL)).clamp(0.0, 1.0)
}

/// Fills a triangle whose `looks` (axis and material per corner) differ.
#[allow(clippy::too_many_arguments)]
pub(super) fn fill_blended(
    fb: &mut Framebuffer,
    c: &[Corner; 3],
    grad: &Gradient,
    looks: [(u8, u8); 3],
    full: bool,
    ctx: &FillContext,
    stats: &mut DrawStats,
) {
    let nearest = c[0].iz.max(c[1].iz).max(c[2].iz);
    let rate = rate_of(ctx.cam.focal(), nearest);
    let scale = bump_scale(rate);
    let look = looks.map(|(axis, material)| {
        let id = u16::from(material);
        let tex = (ctx.texture_of)(id);
        let (u_axis, v_axis) = axis_map(axis);
        let table = ctx.spec.get(usize::from(id)).or(ctx.spec.last());
        Look {
            u_axis,
            v_axis,
            lod: tex.lod_sampler(level_of(tex, rate)),
            smooth: tex.is_filtered(),
            normals: tex
                .normal_view()
                .filter(|_| ctx.bump && full && scale > 0.0),
            spec: table.filter(|t| ctx.specular && full && t.is_visible()),
        }
    });
    let (iz0, izx, izy) = grad.plane([c[0].iz, c[1].iz, c[2].iz]);
    let over = |f: &dyn Fn(&Corner) -> Vec3| [0, 1, 2].map(|k| f(&c[k]) * c[k].iz);
    let pos = grad.plane3(over(&|k| k.pos));
    let light = grad.plane3(over(&|k| k.light));
    let lit_dir = grad.plane3(over(&|k| k.world_dir));
    let normal = grad.plane3(over(&|k| k.normal));
    let weight = grad.plane3(c.map(|k| k.weight));
    let fog = ctx.fog;
    let eye = ctx.eye;
    let shades = look.iter().any(|l| l.normals.is_some() || l.spec.is_some());
    let band = fb.row_range();
    let width = fb.width() as i32;
    each_row(c, band, width, |y, xs, xe| {
        let Some((color, depth)) = fb.row_mut(y) else {
            return;
        };
        let fy = y as f32 + 0.5;
        let mut x = xs;
        while x < xe {
            let n = SPAN_LEN.min(xe - x);
            let iz_at = |px: i32| (iz0 + izx * (px as f32 + 0.5) + izy * fy).max(1e-12);
            let (za, zb) = (1.0 / iz_at(x), 1.0 / iz_at(x + n));
            let inv = 1.0 / n as f32;
            let (p0, p1) = (pos.value(x, y, za), pos.value(x + n, y, zb));
            let (l0, l1) = (light.value(x, y, za), light.value(x + n, y, zb));
            let (dp, dl) = ((p1 - p0) * inv, (l1 - l0) * inv);
            // Only what a corner will read: most far triangles read none of it.
            let frame = shades.then(|| {
                let (d0, d1) = (lit_dir.value(x, y, za), lit_dir.value(x + n, y, zb));
                let (n0, n1) = (normal.value(x, y, za), normal.value(x + n, y, zb));
                let half = |d: Vec3, p: Vec3| {
                    let to_light = d.normalized().unwrap_or(FLAT_NORMAL);
                    half_vector(to_light, (eye - p).normalized().unwrap_or(FLAT_NORMAL))
                };
                let (h0, h1) = (half(d0, p0), half(d1, p1));
                SpanFrame {
                    d0,
                    dd: (d1 - d0) * inv,
                    n0,
                    dn: (n1 - n0) * inv,
                    h0,
                    dh: (h1 - h0) * inv,
                }
            });
            stats.spans += 1;
            for i in 0..n {
                let px = x + i;
                let iz = iz_at(px);
                let slot = px as usize;
                if iz <= depth[slot] {
                    continue;
                }
                depth[slot] = iz;
                let f = i as f32;
                // The weights only choose among corners, so depth does not matter.
                let w = weight.at + weight.dx * (px as f32 + 0.5) + weight.dy * fy;
                let t = (f32::from(BAYER[(y & 3) as usize][(px & 3) as usize]) + 0.5) / 16.0;
                let pick = if w.x > t {
                    0
                } else if w.x + w.y > t {
                    1
                } else {
                    2
                };
                let picked = &look[pick];
                let coords = picked.coords(p0 + dp * f);
                let shading = frame.as_ref().map(|s| s.shade(picked, coords, f, scale));
                let (gain, spec) = shading.unwrap_or((1.0, 0.0));
                let mut k = (l0 + dl * f) * gain;
                if k.x.max(k.y).max(k.z) > KNEE {
                    k = Vec3::new(soft_limit(k.x), soft_limit(k.y), soft_limit(k.z));
                }
                let mut out = mul_rgb3(picked.texel(coords), k, dither_of(px, y));
                if spec > MIN_SPEC {
                    out = add_spec(out, k, spec * SPEC_GAIN);
                }
                if let Some(fog) = fog {
                    let amount = fog.amount(1.0 / iz).clamp(0.0, 256.0) as u32;
                    if amount != 0 {
                        out = lerp_rgb(out, fog.color, amount);
                    }
                }
                color[slot] = out;
                stats.pixels += 1;
            }
            x += n;
        }
    });
}

/// World-space light, normal and half vector over one span.
struct SpanFrame {
    d0: Vec3,
    dd: Vec3,
    n0: Vec3,
    dn: Vec3,
    h0: Vec3,
    dh: Vec3,
}

impl SpanFrame {
    /// The bump's gain and the highlight's strength for a pixel of `look`.
    #[inline]
    fn shade(&self, look: &Look, (fu, fv): (i32, i32), f: f32, scale: f32) -> (f32, f32) {
        let normal = self.n0 + self.dn * f;
        let mut n = FLAT_NORMAL;
        let mut gain = 1.0;
        if let Some(map) = &look.normals {
            n = view_normal(map, fu, fv);
            if scale < 1.0 {
                n = FLAT_NORMAL + (n - FLAT_NORMAL) * scale;
            }
            gain = bump_gain(n, look.to_frame(self.d0 + self.dd * f, normal));
        }
        let spec = look.spec.map_or(0.0, |table| {
            let h = look.to_frame(self.h0 + self.dh * f, normal);
            table.response(n.dot(h) * (1.5 - 0.5 * h.dot(h)))
        });
        (gain, spec)
    }
}
