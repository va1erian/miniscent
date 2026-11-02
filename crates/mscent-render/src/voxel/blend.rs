//! Triangles whose corners disagree on texture axis or material, and those of a
//! material with spots, which this path alone draws.
//!
//! The axis is chosen per pixel from the interpolated normal, so two projections
//! meet along the line where the surface really turns from one to the other. That
//! line is the shape's own and smooth; a choice per corner follows the voxel grid,
//! and draws squares around anything round. In a narrow band around the line the
//! two fade together, as a hard cut shows on anything with contrast, and a dither
//! would turn a pattern to noise. The band fetches twice, on these triangles alone.
//! Two materials meet at the middle of the triangle, with a narrow dither to take
//! off the stair-step.
//!
//! They shade as their neighbours do, or they would show as patches on anything
//! glossy: the highlight is the same sum as a plain triangle's. With no one tangent
//! frame, the light is stepped in world space and moved into the pixel's frame. It
//! costs a few dot products more on these triangles alone.

use mscent_math::Vec3;

use super::fill::FillContext;
use super::shader::{gloss_dot, world_half};
use super::solid::{SPOT_COLOR, Spots, spots_of};
use super::tri::{Corner, Gradient, each_row};
use super::{SEAM_BAND, TEXELS_PER_UNIT, axis_map, seam_of};
use crate::bump::{FLAT_NORMAL, bump_gain, view_normal};
use crate::framebuffer::Framebuffer;
use crate::lighting_shader::{
    BUMP_FULL, BUMP_GONE, KNEE, MIN_SPEC, SPEC_GAIN, add_spec, dither_of, mul_rgb3, soft_limit,
};
use crate::raster::{DrawStats, SPAN_LEN};
use crate::shade::lerp_rgb;
use crate::specular::SpecTable;
use crate::texture::{LodSampler, NormalView, Texture};

/// Thresholds of the ordered dither, as fractions of 16.
const BAYER: [[u8; 4]; 4] = [[0, 8, 2, 10], [12, 4, 14, 6], [3, 11, 1, 9], [15, 7, 13, 5]];

/// How close two corners' weights must be, for a dither to choose between them.
const MATERIAL_BAND: f32 = 0.15;

/// One corner's material.
struct Look<'a> {
    lod: LodSampler<'a>,
    smooth: bool,
    normals: Option<NormalView<'a>>,
    spec: Option<&'a SpecTable>,
    spots: Option<&'static Spots>,
}

/// The two axes that texture a surface facing a given way.
#[derive(Clone, Copy)]
struct Frame {
    u: Vec3,
    v: Vec3,
}

impl Frame {
    #[inline]
    fn of(axis: u8) -> Self {
        let (u, v) = axis_map(axis);
        Self { u, v }
    }

    /// Texel coordinates of `p` in 16.16 fixed point.
    #[inline]
    fn coords(&self, p: Vec3) -> (i32, i32) {
        let fixed = |v: f32| (f64::from(v) * f64::from(TEXELS_PER_UNIT) * 65536.0) as i64 as i32;
        (fixed(p.dot(self.u)), fixed(p.dot(self.v)))
    }

    /// `v` in the frame the normal maps are authored in, on a surface facing `n`.
    #[inline]
    fn to_tangent(self, v: Vec3, n: Vec3) -> Vec3 {
        Vec3::new(v.dot(self.u), -v.dot(self.v), v.dot(n))
    }
}

/// Which corner's material a pixel takes: the corner it is nearest, with a dither
/// only where two are nearly level. `t` is the dither's threshold, in `0..1`.
#[inline]
fn pick_corner(w: Vec3, t: f32) -> usize {
    let w = [w.x, w.y, w.z];
    let (mut first, mut second) = (0, 1);
    if w[1] > w[0] {
        (first, second) = (1, 0);
    }
    if w[2] > w[first] {
        (first, second) = (2, first);
    } else if w[2] > w[second] {
        second = 2;
    }
    let lead = (w[first] - w[second]) / MATERIAL_BAND;
    if lead >= 1.0 || t < 0.5 + 0.5 * lead {
        first
    } else {
        second
    }
}

impl Look<'_> {
    #[inline]
    fn texel(&self, (fu, fv): (i32, i32)) -> u32 {
        if self.smooth {
            self.lod.bilinear(fu, fv)
        } else {
            self.lod.nearest(fu >> 16, fv >> 16)
        }
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
    ctx: &FillContext,
    stats: &mut DrawStats,
) {
    let nearest = c[0].iz.max(c[1].iz).max(c[2].iz);
    let rate = rate_of(ctx.cam.focal(), nearest);
    let scale = bump_scale(rate);
    let one_material = looks[0].1 == looks[1].1 && looks[1].1 == looks[2].1;
    let look = looks.map(|(_, material)| {
        let id = u16::from(material);
        let tex = (ctx.texture_of)(id);
        let table = ctx.spec.get(usize::from(id)).or(ctx.spec.last());
        Look {
            lod: tex.lod_sampler(level_of(tex, rate)),
            smooth: tex.is_filtered(),
            normals: tex.normal_view().filter(|_| ctx.bump && scale > 0.0),
            spec: table.filter(|t| ctx.specular && t.is_visible()),
            spots: spots_of(material),
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
    let focal = ctx.cam.focal();
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
            let (n0, n1) = (normal.value(x, y, za), normal.value(x + n, y, zb));
            let dn = (n1 - n0) * inv;
            // Only what a corner will read: most far triangles read none of it.
            let frame = shades.then(|| {
                let (d0, d1) = (lit_dir.value(x, y, za), lit_dir.value(x + n, y, zb));
                let (h0, h1) = (world_half(d0, p0, eye), world_half(d1, p1, eye));
                SpanFrame {
                    d0,
                    dd: (d1 - d0) * inv,
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
                let surface = n0 + dn * f;
                let (first, second, lead) = seam_of(surface);
                let axes = Frame::of(first);
                // How much of the second projection shows: half where they meet.
                let fade = (lead < SEAM_BAND).then(|| 0.5 * (1.0 - lead / SEAM_BAND));
                let picked = if one_material {
                    &look[0]
                } else {
                    // The weights only choose among corners, so depth does not matter.
                    let w = weight.at + weight.dx * (px as f32 + 0.5) + weight.dy * fy;
                    let t = (f32::from(BAYER[(y & 3) as usize][(px & 3) as usize]) + 0.5) / 16.0;
                    &look[pick_corner(w, t)]
                };
                let coords = axes.coords(p0 + dp * f);
                // The two normal maps do not agree either, so relief fades to flat
                // at the line, where both sides then give the same.
                let relief = fade.map_or(scale, |m| scale * (1.0 - 2.0 * m));
                let shading = frame
                    .as_ref()
                    .map(|s| s.shade(picked, axes, surface, coords, f, relief));
                let (gain, spec) = shading.unwrap_or((1.0, 0.0));
                let mut k = (l0 + dl * f) * gain;
                if k.x.max(k.y).max(k.z) > KNEE {
                    k = Vec3::new(soft_limit(k.x), soft_limit(k.y), soft_limit(k.z));
                }
                let mut texel = picked.texel(coords);
                if let Some(m) = fade {
                    let other = picked.texel(Frame::of(second).coords(p0 + dp * f));
                    texel = lerp_rgb(texel, other, (m * 256.0) as u32);
                }
                if let Some(spots) = picked.spots {
                    let cover = spots.coverage(p0 + dp * f, 1.0 / (focal * iz));
                    texel = lerp_rgb(texel, SPOT_COLOR, (cover * 256.0) as u32);
                }
                let mut out = mul_rgb3(texel, k, dither_of(px, y));
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

/// World-space light and half vector over one span.
struct SpanFrame {
    d0: Vec3,
    dd: Vec3,
    h0: Vec3,
    dh: Vec3,
}

impl SpanFrame {
    /// The bump's gain and the highlight's strength for a pixel of `look`.
    #[inline]
    fn shade(
        &self,
        look: &Look,
        axes: Frame,
        surface: Vec3,
        (fu, fv): (i32, i32),
        f: f32,
        scale: f32,
    ) -> (f32, f32) {
        let mut n = FLAT_NORMAL;
        let mut gain = 1.0;
        if let Some(map) = &look.normals {
            n = view_normal(map, fu, fv);
            if scale < 1.0 {
                n = FLAT_NORMAL + (n - FLAT_NORMAL) * scale;
            }
            gain = bump_gain(n, axes.to_tangent(self.d0 + self.dd * f, surface));
        }
        let spec = look.spec.map_or(0.0, |table| {
            let h = self.h0 + self.dh * f;
            let unit = surface.normalized().unwrap_or(FLAT_NORMAL);
            table.response(gloss_dot(n, h, unit, axes.u, axes.v))
        });
        (gain, spec)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_normal_near_the_line_between_axes_is_in_the_band() {
        let (first, second, lead) = seam_of(Vec3::new(0.5, 0.48, 0.1));
        assert_eq!((first, second), (0, 1));
        assert!(lead < SEAM_BAND);
        let (first, _, lead) = seam_of(Vec3::new(0.1, 0.2, 0.9));
        assert_eq!(first, 2);
        assert!(lead >= SEAM_BAND);
    }

    #[test]
    fn a_clear_lead_takes_no_dither() {
        let w = Vec3::new(0.7, 0.2, 0.1);
        for t in [0.0, 0.5, 0.99] {
            assert_eq!(pick_corner(w, t), 0);
        }
        assert_eq!(pick_corner(Vec3::new(0.1, 0.1, 0.8), 0.99), 2);
    }

    #[test]
    fn nearly_level_corners_take_turns() {
        let w = Vec3::new(0.48, 0.5, 0.02);
        assert_eq!(pick_corner(w, 0.01), 1);
        assert_eq!(pick_corner(w, 0.99), 0);
    }
}
