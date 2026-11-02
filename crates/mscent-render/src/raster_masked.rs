//! The span loop for cutout textures, beside [`crate::raster::span_pixels`]
//! (which opaque textures keep, untouched).

use crate::raster::{Span, fix};
use crate::shade::{SpanShader, lerp_rgb};
use crate::texture::Texture;

/// Like `span_pixels`, but each texel's alpha decides what the pixel does once
/// the depth test has passed: 0 leaves colour and depth alone, 255 writes both,
/// and in between the shaded colour is blended over what is there with depth
/// left alone, so a soft edge never hides what is drawn behind it later.
/// Returns the number of pixels whose colour changed. Not inlined: opaque levels
/// never run it, and inlined it would only grow `draw_row`.
#[inline(never)]
pub(crate) fn masked_span_pixels<S: SpanShader>(
    color: &mut [u32],
    depth: &mut [f32],
    s: &Span,
    tex: &Texture,
    shader: &S,
) -> u32 {
    let step = 1.0 / s.n as f32;
    let (mut fu, mut fv) = (fix(s.ea.u), fix(s.ea.v));
    let (du, dv) = (fix((s.eb.u - s.ea.u) * step), fix((s.eb.v - s.ea.v) * step));
    let lod = tex.lod_sampler(s.level);
    // A masked texture always has its plane; the fallback is a solid one.
    let alpha = tex.alpha_sampler(s.level);
    let mut written = 0;
    for i in 0..s.n {
        let iz = s.iz0 + s.diz * i as f32;
        let slot = s.x + i;
        if iz > depth[slot] {
            let a = match &alpha {
                None => u8::MAX,
                Some(p) if s.smooth => p.bilinear(fu, fv),
                Some(p) => p.nearest(fu >> 16, fv >> 16),
            };
            if a != 0 {
                let texel = if s.smooth {
                    lod.bilinear(fu, fv)
                } else {
                    lod.nearest(fu >> 16, fv >> 16)
                };
                let shaded = shader.shade(i as u32, texel, fu, fv);
                if a == u8::MAX {
                    depth[slot] = iz;
                    color[slot] = shaded;
                } else {
                    // 0..=255 onto 0..=256 so the top end is not a step short.
                    let t = u32::from(a) + u32::from(a >> 7);
                    color[slot] = lerp_rgb(color[slot], shaded, t);
                }
                written += 1;
            }
        }
        fu = fu.wrapping_add(du);
        fv = fv.wrapping_add(dv);
    }
    written
}

#[cfg(test)]
#[path = "raster_masked_tests.rs"]
mod raster_masked_tests;
