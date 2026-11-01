//! Per-span shading: the hook where lighting plugs into the rasterizer.
//!
//! The rasterizer walks each scanline in spans of up to 16 pixels and
//! evaluates the perspective-correct attributes only at the span ends. A
//! [`SpanShader`] sees both ends once per span ([`SpanShader::begin_span`]),
//! derives whatever it wants to step linearly (light colour, fog, a tangent
//! space light direction), and is then asked for each pixel's colour by its
//! index within the span. Lightmap and bump shaders are written against this
//! trait without touching the rasterizer.

/// Perspective-correct attributes at one end of a span.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpanEnd {
    /// Screen position of the end (pixel index along the row).
    pub x: i32,
    pub y: i32,
    /// Texture coordinates in texels (unwrapped).
    pub u: f32,
    pub v: f32,
    /// View depth (distance along the view axis), not its reciprocal.
    pub depth: f32,
}

/// Colours a pixel from its texel and span-local state.
pub trait SpanShader {
    /// Called once per span before its pixels. `len` pixels follow, with
    /// indices `0..len`. `start` and `end` are sampled at the centre of pixel
    /// 0 and of pixel `len` (one past the last), so attribute `a` at pixel
    /// `i` is `start + (end - start) * i / len`.
    fn begin_span(&mut self, start: &SpanEnd, end: &SpanEnd, len: u32);

    /// Final `0xAARRGGBB` colour of pixel `i` of the current span, given its
    /// albedo `texel` and its unwrapped texel coordinates `(fu, fv)` in 16.16
    /// fixed point (texel `n` starts at `n << 16`).
    fn shade(&self, i: u32, texel: u32, fu: i32, fv: i32) -> u32;
}

/// Linear fog between two view depths.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Fog {
    pub color: u32,
    start: f32,
    end: f32,
}

impl Fog {
    /// `None` unless `0 <= start < end` and both are finite.
    pub fn new(color: u32, start: f32, end: f32) -> Option<Self> {
        let ok = start.is_finite() && end.is_finite() && start >= 0.0 && end > start;
        ok.then_some(Self { color, start, end })
    }

    /// Fog amount at `depth` on a 0 (clear) to 256 (fully fogged) scale.
    pub fn amount(&self, depth: f32) -> f32 {
        ((depth - self.start) / (self.end - self.start)).clamp(0.0, 1.0) * 256.0
    }
}

/// Constant light plus optional fog: the flat-lit fallback used when a face
/// has no lightmap, and the base that fog adds to every other shader.
#[derive(Clone, Copy, Debug)]
pub struct FlatShader {
    /// Light scale, 256 = unchanged; values above 256 over-brighten.
    pub light: u32,
    pub fog: Option<Fog>,
    fog0: f32,
    fog_step: f32,
}

impl FlatShader {
    pub fn new(light: u32, fog: Option<Fog>) -> Self {
        Self {
            light,
            fog,
            fog0: 0.0,
            fog_step: 0.0,
        }
    }
}

impl SpanShader for FlatShader {
    fn begin_span(&mut self, start: &SpanEnd, end: &SpanEnd, len: u32) {
        if let Some(fog) = self.fog {
            self.fog0 = fog.amount(start.depth);
            self.fog_step = (fog.amount(end.depth) - self.fog0) / len.max(1) as f32;
        }
    }

    fn shade(&self, i: u32, texel: u32, _fu: i32, _fv: i32) -> u32 {
        let lit = mul_rgb(texel, self.light);
        match self.fog {
            Some(fog) => {
                let amount = (self.fog0 + self.fog_step * i as f32).clamp(0.0, 256.0);
                lerp_rgb(lit, fog.color, amount as u32)
            }
            None => lit,
        }
    }
}

/// Scale the colour channels by `k / 256`, saturating at 255. Alpha is
/// forced opaque.
pub fn mul_rgb(c: u32, k: u32) -> u32 {
    let ch = |shift: u32| (((c >> shift) & 0xff).saturating_mul(k) >> 8).min(255);
    0xff00_0000 | ch(16) << 16 | ch(8) << 8 | ch(0)
}

/// Blend `a` toward `b` by `t / 256` (`t` is clamped to 256). Alpha is forced
/// opaque.
pub fn lerp_rgb(a: u32, b: u32, t: u32) -> u32 {
    let t = t.min(256) as i32;
    let ch = |shift: u32| {
        let (x, y) = (((a >> shift) & 0xff) as i32, ((b >> shift) & 0xff) as i32);
        (x + (((y - x) * t) >> 8)) as u32
    };
    0xff00_0000 | ch(16) << 16 | ch(8) << 8 | ch(0)
}

#[cfg(test)]
#[path = "shade_tests.rs"]
mod shade_tests;
