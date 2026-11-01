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
use mscent_world::lightmap::{FaceGrid, FaceProjection, Lightmap, Luxel};

use crate::bump::{FLAT_NORMAL, bump_gain, texel_normal};
use crate::raster::Vertex;
use crate::shade::{Fog, SpanEnd, SpanShader, lerp_rgb};
use crate::specular::{SpecTable, half_vector};
use crate::texture::Texture;

/// Highlight strength: the table peaks at gloss/255 of the light colour,
/// which on dim baked light would be invisible.
const SPEC_GAIN: f32 = 1.1;

/// Table response below which a highlight adds nothing visible (the light
/// level tops out near 300 of 256, so 1/300 is under one level).
const MIN_SPEC: f32 = 1.0 / 300.0;

/// One fitting sample: texel `(u, v)` paired with luxel `(lx, ly)`.
type Pair = [f32; 4];

/// Affine map from unwrapped texel coordinates to continuous luxel
/// coordinates of one face (luxel `i` covers `i..i + 1`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct UvToLuxel {
    m: [f32; 6],
    /// Unit vectors, in the tangent frame's xy plane, of the texture's +u
    /// axis and of its up axis (-v, the normal map's +y).
    frame: [f32; 4],
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
        let unit = |x: f32, y: f32| {
            let len = x.hypot(y);
            (len > 0.0).then(|| [x / len, y / len])
        };
        let (eu, ev) = (unit(a, d)?, unit(-b, -e)?);
        let frame = [eu[0], eu[1], ev[0], ev[1]];
        (m.iter().chain(&frame).all(|x| x.is_finite())).then_some(Self { m, frame })
    }

    /// Re-express a unit vector given in the lightmap's tangent frame in the
    /// texture's frame (x along +u, y towards the top of the image), which is
    /// the frame the normal maps are authored in. The two frames differ by a
    /// rotation (and possibly a mirror) that depends on the face orientation.
    pub fn to_texture_frame(&self, v: Vec3) -> Vec3 {
        let f = &self.frame;
        Vec3::new(v.x * f[0] + v.y * f[1], v.x * f[2] + v.y * f[3], v.z)
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
    grid: FaceGrid<'a>,
    proj: FaceProjection,
    spec: Option<SpecInput<'a>>,
    // Per-span state. The lightmap is sampled every `1 << shift` pixels and
    // linear between those knots; colour is in 256 = full-light units.
    segs: [Seg; MAX_SEGS],
    shift: u32,
    /// How much of the bump effect survives at the current minification.
    bump_scale: f32,
    /// Screen position of pixel 0 of the span, for the dither.
    x0: i32,
    y0: i32,
    fog0: f32,
    fog_step: f32,
}

/// Longest lightmap sampling interval, in pixels (a power of two).
const MAX_SEG_SHIFT: u32 = 4;
/// Shortest sampling interval, as a shift: every second pixel.
const MIN_SEG_SHIFT: u32 = 1;
/// Segments of the longest span (`SPAN_LEN >> MIN_SEG_SHIFT`).
const MAX_SEGS: usize = 8;
/// Largest distance, in luxels, between two lightmap samples on screen.
/// Beyond one luxel the linear step between knots would skip over the
/// penumbra of a shadow edge, and the edge would crawl with the span grid.
const MAX_KNOT_LUXELS: f32 = 2.0;
/// Texels per pixel up to which bump mapping keeps full strength, and the
/// rate at which it has faded out (the normal map aliases beyond that).
const BUMP_FULL: f32 = 1.0;
const BUMP_GONE: f32 = 3.0;

/// Linear light state over one sampling interval.
#[derive(Clone, Copy)]
struct Seg {
    col0: Vec3,
    dcol: Vec3,
    dir0: Vec3,
    ddir: Vec3,
    half0: Vec3,
    dhalf: Vec3,
}

/// Light at one lightmap sample.
#[derive(Clone, Copy)]
struct Knot {
    col: Vec3,
    dir: Vec3,
    half: Vec3,
}

const EMPTY_SEG: Seg = Seg {
    col0: Vec3::ZERO,
    dcol: Vec3::ZERO,
    dir0: FLAT_NORMAL,
    ddir: Vec3::ZERO,
    half0: FLAT_NORMAL,
    dhalf: Vec3::ZERO,
};

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
        let grid = face.lightmap.face_grid(face.face)?;
        Some(Self {
            face,
            grid,
            proj,
            spec,
            segs: [EMPTY_SEG; MAX_SEGS],
            shift: MAX_SEG_SHIFT,
            bump_scale: 1.0,
            x0: 0,
            y0: 0,
            fog0: 0.0,
            fog_step: 0.0,
        })
    }

    /// Whether anything downstream reads the baked light direction.
    fn wants_dir(&self) -> bool {
        self.face.bump || self.spec.is_some()
    }

    /// Bilinear luxel sample at continuous luxel coordinates, clamped to the
    /// face's own grid so neighbouring faces in the atlas never bleed in. The
    /// direction is blended once and normalised once.
    fn sample(&self, lx: f32, ly: f32) -> Luxel {
        let g = &self.grid;
        let clamp = |c: f32, n: u32| (c - 0.5).clamp(0.0, n.saturating_sub(1) as f32);
        let (fx, fy) = (clamp(lx, g.width()), clamp(ly, g.height()));
        let (x0, y0) = (fx as u32, fy as u32);
        let (tx, ty) = (fx - x0 as f32, fy - y0 as f32);
        let taps = [
            (g.at_clamped(x0, y0), (1.0 - tx) * (1.0 - ty)),
            (g.at_clamped(x0 + 1, y0), tx * (1.0 - ty)),
            (g.at_clamped(x0, y0 + 1), (1.0 - tx) * ty),
            (g.at_clamped(x0 + 1, y0 + 1), tx * ty),
        ];
        let color = taps.iter().fold(Vec3::ZERO, |a, (l, w)| a + l.color * *w);
        if !self.wants_dir() {
            return Luxel {
                color,
                dir: Luxel::FALLBACK_DIR,
            };
        }
        let dir = taps.iter().fold(Vec3::ZERO, |a, (l, w)| a + l.dir * *w);
        Luxel {
            color,
            dir: dir.normalized().unwrap_or(Luxel::FALLBACK_DIR),
        }
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

    /// Light at luxel coordinates `(lx, ly)`.
    fn knot(&self, lx: f32, ly: f32) -> Knot {
        let l = self.sample(lx, ly);
        // Bump normals live in the texture frame; the baked directions and
        // the view vector are in the lightmap's tangent frame.
        let to_tex = |v: Vec3| self.face.map.to_texture_frame(v);
        if !self.wants_dir() {
            return Knot {
                col: l.color * 256.0,
                dir: FLAT_NORMAL,
                half: FLAT_NORMAL,
            };
        }
        let half = match self.spec {
            Some(spec) => to_tex(half_vector(l.dir, self.view_dir(&spec, lx, ly))),
            None => FLAT_NORMAL,
        };
        Knot {
            col: l.color * 256.0,
            dir: to_tex(l.dir),
            half,
        }
    }

    /// Sample the lightmap along the span every `1 << shift` pixels, where
    /// the shift keeps the knots about a luxel apart on the face. Luxel
    /// coordinates are affine in the span's texture coordinates, so knots are
    /// placed by linear interpolation of the two ends (evaluated at pixel
    /// centres, so they do not depend on where the span grid starts).
    fn begin_light(&mut self, start: &SpanEnd, end: &SpanEnd, len: u32) {
        let (ax, ay) = self.face.map.apply(start.u, start.v);
        let (bx, by) = self.face.map.apply(end.u, end.v);
        let per_px = (bx - ax).abs().max((by - ay).abs()) / len.max(1) as f32;
        self.shift = MAX_SEG_SHIFT;
        while self.shift > MIN_SEG_SHIFT && (1u32 << self.shift) as f32 * per_px > MAX_KNOT_LUXELS {
            self.shift -= 1;
        }
        let seg_len = 1u32 << self.shift;
        let t_per_px = 1.0 / len.max(1) as f32;
        let inv = 1.0 / seg_len as f32;
        let mut a = self.knot(ax, ay);
        for k in 0..len.div_ceil(seg_len) {
            let t = ((k + 1) * seg_len) as f32 * t_per_px;
            let b = self.knot(ax + (bx - ax) * t, ay + (by - ay) * t);
            self.segs[k as usize & (MAX_SEGS - 1)] = Seg {
                col0: a.col,
                dcol: (b.col - a.col) * inv,
                dir0: a.dir,
                ddir: (b.dir - a.dir) * inv,
                half0: a.half,
                dhalf: (b.half - a.half) * inv,
            };
            a = b;
        }
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
    // KNEE + d / (1 + d / ROLL) with d = x - KNEE, rearranged to x - d^2 /
    // (ROLL + d) so there is no branch: d is zero below the knee.
    let d = (x - KNEE).max(0.0);
    x - d * d / (ROLL + d)
}

/// Cheap noise in `[0, 1)` from the screen position. It is fixed on the
/// screen, so it cannot crawl across surfaces as the camera moves (noise
/// keyed by span index or texel does, since both shift every frame).
fn dither_of(x: i32, y: i32) -> f32 {
    let h = (x as u32).wrapping_mul(0x9E37_79B1) ^ (y as u32).wrapping_mul(0x85EB_CA6B);
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
    fn set_texel_rate(&mut self, texels_per_pixel: f32) {
        let t = (BUMP_GONE - texels_per_pixel) / (BUMP_GONE - BUMP_FULL);
        self.bump_scale = if t.is_nan() { 0.0 } else { t.clamp(0.0, 1.0) };
    }

    fn begin_span(&mut self, start: &SpanEnd, end: &SpanEnd, len: u32) {
        self.begin_light(start, end, len);
        (self.x0, self.y0) = (start.x, start.y);
        if let Some(fog) = self.face.fog {
            let inv = 1.0 / len.max(1) as f32;
            self.fog0 = fog.amount(start.depth);
            self.fog_step = (fog.amount(end.depth) - self.fog0) * inv;
        }
    }

    fn shade(&self, i: u32, texel: u32, fu: i32, fv: i32) -> u32 {
        let seg = &self.segs[(i >> self.shift) as usize & (MAX_SEGS - 1)];
        let f = (i & ((1 << self.shift) - 1)) as f32;
        let mut k = seg.col0 + seg.dcol * f;
        let mut n = FLAT_NORMAL;
        // Past BUMP_GONE the relief has faded to flat, whose gain is exactly
        // 1, so the normal fetch is skipped.
        if self.face.bump && self.bump_scale > 0.0 {
            // Fade the normal towards flat as the map is minified.
            n = texel_normal(self.face.texture, fu, fv);
            if self.bump_scale < 1.0 {
                n = FLAT_NORMAL + (n - FLAT_NORMAL) * self.bump_scale;
            }
            k = k * bump_gain(n, seg.dir0 + seg.ddir * f);
        }
        // Below the knee the roll-off is the identity: skip its divisions.
        if k.x.max(k.y).max(k.z) > KNEE {
            k = Vec3::new(soft_limit(k.x), soft_limit(k.y), soft_limit(k.z));
        }
        let dither = dither_of(self.x0 + i as i32, self.y0);
        let mut out = mul_rgb3(texel, k, dither);
        if let Some(spec) = self.spec.filter(|_| SPEC) {
            let h = seg.half0 + seg.dhalf * f;
            // Stepping unit vectors linearly shortens them mid-span, which
            // would flatten the peak; one Newton step of 1/|h| restores it.
            let n_dot_h = n.dot(h) * (1.5 - 0.5 * h.dot(h));
            let s = spec.table.response(n_dot_h);
            // Most pixels are outside the lobe; below this the add is under
            // one level for any light the shader can produce.
            if s > MIN_SPEC {
                out = add_spec(out, k, s * SPEC_GAIN);
            }
        }
        match self.face.fog {
            Some(fog) => {
                let amount = (self.fog0 + self.fog_step * i as f32).clamp(0.0, 256.0);
                lerp_rgb(out, fog.color, amount as u32)
            }
            None => out,
        }
    }
}

#[cfg(test)]
#[path = "lighting_shader_tests.rs"]
mod lighting_shader_tests;
