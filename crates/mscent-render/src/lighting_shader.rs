//! Lightmap shader: baked colour and light direction, stepped across spans.
//!
//! The lightmap is sampled twice per 16-pixel span (bilinear, at the span
//! ends), and colour and tangent-space light direction are stepped linearly
//! between the two, exactly like the texture coordinates. Per pixel the cost
//! is a texel multiply; bump mapping adds a normal fetch and one dot product
//! ([`crate::bump`]); specular ([`crate::specular`]) adds one more dot and a
//! table lookup, and only exists in the `SPEC = true` instantiation, so the
//! default path pays nothing for it.

use mscent_math::Vec3;
use mscent_world::lightmap::{FaceProjection, Lightmap, Luxel};

use crate::bump::{FLAT_NORMAL, bump_gain, texel_normal};
use crate::raster::Vertex;
use crate::shade::{Fog, SpanEnd, SpanShader, lerp_rgb};
use crate::specular::{SpecTable, half_vector};
use crate::texture::Texture;

/// Highlight strength: the table peaks at gloss/255 of the light colour,
/// which on dim baked light would be invisible.
const SPEC_GAIN: f32 = 1.1;

/// One fitting sample: texel `(u, v)` paired with luxel `(lx, ly)`.
type Pair = [f32; 4];

/// Affine map from unwrapped texel coordinates to continuous luxel
/// coordinates of one face (luxel `i` covers `i..i + 1`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct UvToLuxel {
    m: [f32; 6],
}

impl UvToLuxel {
    /// Fit the map from a face's polygon: each vertex's world position gives
    /// its luxel coordinates through `proj`, each vertex's `uv` (times the
    /// texture size) gives the texel coordinates the rasterizer will see.
    /// `None` if the polygon has no area in uv space or a value is not finite.
    pub fn from_vertices(proj: &FaceProjection, verts: &[Vertex], tex: &Texture) -> Option<Self> {
        let (tw, th) = (tex.width() as f32, tex.height() as f32);
        let pts: Vec<Pair> = verts
            .iter()
            .map(|v| {
                let d = v.pos - proj.origin;
                let lx = (d.dot(proj.tangent) - proj.u_min) / proj.luxel_size;
                let ly = (d.dot(proj.bitangent) - proj.v_min) / proj.luxel_size;
                [v.uv.x * tw, v.uv.y * th, lx, ly]
            })
            .collect();
        let (p0, p1, p2) = widest_triangle(&pts)?;
        let det = (p1[0] - p0[0]) * (p2[1] - p0[1]) - (p1[1] - p0[1]) * (p2[0] - p0[0]);
        let solve = |k: usize| {
            let (d1, d2) = (p1[k] - p0[k], p2[k] - p0[k]);
            let a = (d1 * (p2[1] - p0[1]) - (p1[1] - p0[1]) * d2) / det;
            let b = ((p1[0] - p0[0]) * d2 - d1 * (p2[0] - p0[0])) / det;
            (a, b, p0[k] - a * p0[0] - b * p0[1])
        };
        let (a, b, c) = solve(2);
        let (d, e, f) = solve(3);
        let m = [a, b, c, d, e, f];
        m.iter().all(|x| x.is_finite()).then_some(Self { m })
    }

    /// Luxel coordinates of texel coordinates `(u, v)`.
    pub fn apply(&self, u: f32, v: f32) -> (f32, f32) {
        let m = &self.m;
        (m[0] * u + m[1] * v + m[2], m[3] * u + m[4] * v + m[5])
    }
}

/// Three points of the largest uv-space area, the best-conditioned basis.
fn widest_triangle(pts: &[Pair]) -> Option<(Pair, Pair, Pair)> {
    let p0 = *pts.first()?;
    let dist = |p: &Pair| (p[0] - p0[0]).powi(2) + (p[1] - p0[1]).powi(2);
    let p1 = *pts.iter().max_by(|a, b| dist(a).total_cmp(&dist(b)))?;
    let area =
        |p: &Pair| ((p1[0] - p0[0]) * (p[1] - p0[1]) - (p1[1] - p0[1]) * (p[0] - p0[0])).abs();
    let p2 = *pts.iter().max_by(|a, b| area(a).total_cmp(&area(b)))?;
    (area(&p2) > 1e-9).then_some((p0, p1, p2))
}

/// Everything the shader needs about the face being drawn.
#[derive(Clone, Copy)]
pub struct FaceLight<'a> {
    pub lightmap: &'a Lightmap,
    /// Index of the face in the baked lightmap.
    pub face: usize,
    pub map: UvToLuxel,
    /// The albedo texture the rasterizer is sampling; its normal map drives
    /// bump mapping.
    pub texture: &'a Texture,
    pub fog: Option<Fog>,
    /// Apply the texture's normal map (no effect without one).
    pub bump: bool,
}

/// Eye position and power table for the specular path.
#[derive(Clone, Copy)]
struct SpecInput<'a> {
    /// Eye in the face tangent frame (origin at the face frame's origin).
    eye_t: Vec3,
    table: &'a SpecTable,
}

/// Lightmap, bump and optional specular in one span shader.
pub struct LightingShader<'a, const SPEC: bool> {
    face: FaceLight<'a>,
    proj: FaceProjection,
    spec: Option<SpecInput<'a>>,
    // Per-span linear state; colour is in 256 = full-light units.
    col0: Vec3,
    dcol: Vec3,
    dir0: Vec3,
    ddir: Vec3,
    half0: Vec3,
    dhalf: Vec3,
    fog0: f32,
    fog_step: f32,
}

impl<'a> LightingShader<'a, false> {
    /// Diffuse-only shader. `None` if `face.face` is not in the lightmap.
    pub fn new(face: FaceLight<'a>) -> Option<Self> {
        Self::build(face, None)
    }
}

impl<'a> LightingShader<'a, true> {
    /// Shader with specular highlights for an eye at `eye` (world space) and
    /// the material's power table. `None` if `face.face` is not in the
    /// lightmap.
    pub fn with_specular(face: FaceLight<'a>, eye: Vec3, table: &'a SpecTable) -> Option<Self> {
        let proj = face.lightmap.projections().get(face.face)?;
        let eye_t = proj.to_tangent(eye - proj.origin);
        Self::build(face, Some(SpecInput { eye_t, table }))
    }
}

impl<'a, const SPEC: bool> LightingShader<'a, SPEC> {
    fn build(face: FaceLight<'a>, spec: Option<SpecInput<'a>>) -> Option<Self> {
        let proj = *face.lightmap.projections().get(face.face)?;
        Some(Self {
            face,
            proj,
            spec,
            col0: Vec3::ZERO,
            dcol: Vec3::ZERO,
            dir0: FLAT_NORMAL,
            ddir: Vec3::ZERO,
            half0: FLAT_NORMAL,
            dhalf: Vec3::ZERO,
            fog0: 0.0,
            fog_step: 0.0,
        })
    }

    /// Bilinear luxel sample at continuous luxel coordinates, clamped to the
    /// face's own grid so neighbouring faces in the atlas never bleed in.
    fn sample(&self, lx: f32, ly: f32) -> Luxel {
        let (w, h) = (self.proj.width, self.proj.height);
        let clamp = |c: f32, n: u32| (c - 0.5).clamp(0.0, n.saturating_sub(1) as f32);
        let (fx, fy) = (clamp(lx, w), clamp(ly, h));
        let (x0, y0) = (fx as u32, fy as u32);
        let x1 = (x0 + 1).min(w.saturating_sub(1));
        let y1 = (y0 + 1).min(h.saturating_sub(1));
        let lm = self.face.lightmap;
        let at = |x, y| lm.face_luxel(self.face.face, x, y).unwrap_or(Luxel::DARK);
        let (tx, ty) = (fx - x0 as f32, fy - y0 as f32);
        let top = blend(at(x0, y0), at(x1, y0), tx);
        let bottom = blend(at(x0, y1), at(x1, y1), tx);
        blend(top, bottom, ty)
    }

    /// Unit direction from a point on the face (luxel coordinates) to the eye.
    fn view_dir(&self, spec: &SpecInput, lx: f32, ly: f32) -> Vec3 {
        let p = &self.proj;
        let at = Vec3::new(
            p.u_min + lx * p.luxel_size,
            p.v_min + ly * p.luxel_size,
            0.0,
        );
        (spec.eye_t - at).normalized().unwrap_or(FLAT_NORMAL)
    }

    fn begin_light(&mut self, start: &SpanEnd, end: &SpanEnd, inv: f32) {
        let (ax, ay) = self.face.map.apply(start.u, start.v);
        let (bx, by) = self.face.map.apply(end.u, end.v);
        let (a, b) = (self.sample(ax, ay), self.sample(bx, by));
        self.col0 = a.color * 256.0;
        self.dcol = (b.color - a.color) * (256.0 * inv);
        self.dir0 = a.dir;
        self.ddir = (b.dir - a.dir) * inv;
        if let Some(spec) = self.spec {
            let ha = half_vector(a.dir, self.view_dir(&spec, ax, ay));
            let hb = half_vector(b.dir, self.view_dir(&spec, bx, by));
            self.half0 = ha;
            self.dhalf = (hb - ha) * inv;
        }
    }
}

fn blend(a: Luxel, b: Luxel, t: f32) -> Luxel {
    Luxel {
        color: a.color.lerp(b.color, t),
        dir: a
            .dir
            .lerp(b.dir, t)
            .normalized()
            .unwrap_or(Luxel::FALLBACK_DIR),
    }
}

/// Multiply the colour channels by per-channel light `k` (256 = unchanged).
/// Negative or NaN products saturate to zero in the float-to-int cast.
/// `dither` in `[0, 1)` is added before truncation so smooth gradients do not
/// quantise into visible rings.
fn mul_rgb3(c: u32, k: Vec3, dither: f32) -> u32 {
    let ch =
        |shift: u32, k: f32| (((c >> shift) & 0xff) as f32 * k * (1.0 / 256.0) + dither) as u32;
    0xff00_0000 | ch(16, k.x).min(255) << 16 | ch(8, k.y).min(255) << 8 | ch(0, k.z).min(255)
}

/// Light level (256 = full) above which [`soft_limit`] starts rolling off.
const KNEE: f32 = 190.0;
/// Distance past the knee at which the roll-off reaches its ceiling.
const ROLL: f32 = 110.0;

/// Smoothly compresses light above the knee towards `KNEE + ROLL`, so bright
/// pools keep texture detail instead of clipping to flat white.
fn soft_limit(x: f32) -> f32 {
    if x <= KNEE {
        x
    } else {
        let d = x - KNEE;
        KNEE + d / (1.0 + d / ROLL)
    }
}

/// Cheap per-texel noise in `[0, 1)` from the texture position.
fn dither_of(fu: i32, fv: i32, i: u32) -> f32 {
    let h = (fu as u32).wrapping_mul(0x9E37_79B1)
        ^ (fv as u32).wrapping_mul(0x85EB_CA6B)
        ^ i.wrapping_mul(0xC2B2_AE35);
    let h = (h ^ (h >> 15)).wrapping_mul(0x2C1B_3C6D);
    ((h >> 8) & 0xffff) as f32 * (1.0 / 65536.0)
}

/// Add a white highlight of strength `s` scaled by the light colour `k`,
/// saturating per channel.
fn add_spec(c: u32, k: Vec3, s: f32) -> u32 {
    // The float cast saturates at u32::MAX, so the add must saturate too.
    let ch = |shift: u32, k: f32| (((c >> shift) & 0xff).saturating_add((k * s) as u32)).min(255);
    0xff00_0000 | ch(16, k.x) << 16 | ch(8, k.y) << 8 | ch(0, k.z)
}

impl<const SPEC: bool> SpanShader for LightingShader<'_, SPEC> {
    fn begin_span(&mut self, start: &SpanEnd, end: &SpanEnd, len: u32) {
        let inv = 1.0 / len.max(1) as f32;
        self.begin_light(start, end, inv);
        if let Some(fog) = self.face.fog {
            self.fog0 = fog.amount(start.depth);
            self.fog_step = (fog.amount(end.depth) - self.fog0) * inv;
        }
    }

    fn shade(&self, i: u32, texel: u32, fu: i32, fv: i32) -> u32 {
        let f = i as f32;
        let mut k = self.col0 + self.dcol * f;
        let mut n = FLAT_NORMAL;
        if self.face.bump {
            n = texel_normal(self.face.texture, fu, fv);
            k = k * bump_gain(n, self.dir0 + self.ddir * f);
        }
        k = Vec3::new(soft_limit(k.x), soft_limit(k.y), soft_limit(k.z));
        let mut out = mul_rgb3(texel, k, dither_of(fu, fv, i));
        if let Some(spec) = self.spec.filter(|_| SPEC) {
            let h = self.half0 + self.dhalf * f;
            // Stepping unit vectors linearly shortens them mid-span, which
            // would flatten the peak; one Newton step of 1/|h| restores it.
            let n_dot_h = n.dot(h) * (1.5 - 0.5 * h.dot(h));
            let s = spec.table.response(n_dot_h);
            out = add_spec(out, k, s * SPEC_GAIN);
        }
        match self.face.fog {
            Some(fog) => {
                let amount = (self.fog0 + self.fog_step * f).clamp(0.0, 256.0);
                lerp_rgb(out, fog.color, amount as u32)
            }
            None => out,
        }
    }
}

#[cfg(test)]
#[path = "lighting_shader_tests.rs"]
mod lighting_shader_tests;
