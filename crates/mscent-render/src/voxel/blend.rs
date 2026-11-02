//! Triangles whose corners disagree on texture axis or material. Each pixel picks
//! one corner's look through a 4x4 ordered dither fixed on the screen: stable,
//! cheap, and only these triangles pay. The one mechanism serves both cases. They
//! take Gouraud light and albedo only, as their corners have no one tangent frame.

use mscent_math::Vec3;

use super::fill::FillContext;
use super::tri::{Corner, Gradient, each_row};
use super::{TEXELS_PER_UNIT, axis_map};
use crate::framebuffer::Framebuffer;
use crate::lighting_shader::{KNEE, dither_of, mul_rgb3, soft_limit};
use crate::raster::{DrawStats, SPAN_LEN};
use crate::shade::lerp_rgb;
use crate::texture::{LodSampler, Texture};

/// Thresholds of the ordered dither, as fractions of 16.
const BAYER: [[u8; 4]; 4] = [[0, 8, 2, 10], [12, 4, 14, 6], [3, 11, 1, 9], [15, 7, 13, 5]];

/// One corner's look.
struct Look<'a> {
    u_axis: Vec3,
    v_axis: Vec3,
    lod: LodSampler<'a>,
    smooth: bool,
}

impl Look<'_> {
    #[inline]
    fn texel(&self, p: Vec3) -> u32 {
        let fixed = |v: f32| (f64::from(v) * f64::from(TEXELS_PER_UNIT) * 65536.0) as i64 as i32;
        let (fu, fv) = (fixed(p.dot(self.u_axis)), fixed(p.dot(self.v_axis)));
        if self.smooth {
            self.lod.bilinear(fu, fv)
        } else {
            self.lod.nearest(fu >> 16, fv >> 16)
        }
    }
}

/// The mip level of a triangle: all corners share the texel density, so the
/// footprint of its depth is enough.
fn level_of(tex: &Texture, cam_focal: f32, iz: f32) -> u32 {
    let rate = TEXELS_PER_UNIT / (cam_focal * iz.max(1e-6));
    if tex.mip_count() == 0 || rate.is_nan() || rate <= 1.5 {
        return 0;
    }
    ((rate / 1.5).log2().ceil() as u32).min(tex.mip_count())
}

/// Fills a triangle whose `looks` (axis and material per corner) differ.
pub(super) fn fill_blended(
    fb: &mut Framebuffer,
    c: &[Corner; 3],
    grad: &Gradient,
    looks: [(u8, u8); 3],
    ctx: &FillContext,
    stats: &mut DrawStats,
) {
    let nearest = c[0].iz.max(c[1].iz).max(c[2].iz);
    let focal = ctx.cam.focal();
    let look = looks.map(|(axis, material)| {
        let tex = (ctx.texture_of)(u16::from(material));
        let (u_axis, v_axis) = axis_map(axis);
        Look {
            u_axis,
            v_axis,
            lod: tex.lod_sampler(level_of(tex, focal, nearest)),
            smooth: tex.is_filtered(),
        }
    });
    let (iz0, izx, izy) = grad.plane([c[0].iz, c[1].iz, c[2].iz]);
    let over = |f: &dyn Fn(&Corner) -> Vec3| [0, 1, 2].map(|k| f(&c[k]) * c[k].iz);
    let pos = grad.plane3(over(&|k| k.pos));
    let light = grad.plane3(over(&|k| k.light));
    let weight = grad.plane3(c.map(|k| k.weight));
    let fog = ctx.fog;
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
                let texel = look[pick].texel(p0 + dp * f);
                let mut k = l0 + dl * f;
                if k.x.max(k.y).max(k.z) > KNEE {
                    k = Vec3::new(soft_limit(k.x), soft_limit(k.y), soft_limit(k.z));
                }
                let mut out = mul_rgb3(texel, k, dither_of(px, y));
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
