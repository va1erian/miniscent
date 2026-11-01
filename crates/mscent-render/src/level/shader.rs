//! One shader type for every face of a level frame, so a single closure can
//! pick lightmap, lightmap plus specular, or flat emissive per face.

use crate::lighting_shader::LightingShader;
use crate::shade::{FlatShader, SpanEnd, SpanShader};

pub(super) enum FaceShader<'a> {
    Lit(LightingShader<'a, false>),
    Spec(LightingShader<'a, true>),
    Flat(FlatShader),
}

impl SpanShader for FaceShader<'_> {
    fn set_texel_rate(&mut self, rate: f32) {
        match self {
            Self::Lit(s) => s.set_texel_rate(rate),
            Self::Spec(s) => s.set_texel_rate(rate),
            Self::Flat(_) => {}
        }
    }

    fn begin_span(&mut self, start: &SpanEnd, end: &SpanEnd, len: u32) {
        match self {
            Self::Lit(s) => s.begin_span(start, end, len),
            Self::Spec(s) => s.begin_span(start, end, len),
            Self::Flat(s) => s.begin_span(start, end, len),
        }
    }

    fn shade(&self, i: u32, texel: u32, fu: i32, fv: i32) -> u32 {
        match self {
            Self::Lit(s) => s.shade(i, texel, fu, fv),
            Self::Spec(s) => s.shade(i, texel, fu, fv),
            Self::Flat(s) => s.shade(i, texel, fu, fv),
        }
    }
}
