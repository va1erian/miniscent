//! Lightmap shader: baked colour and light direction, fetched per pixel.
//!
//! Luxel coordinates are affine across a span (exact at the span ends,
//! evaluated at pixel centres), so every pixel gets its own bilinear fetch of
//! the lightmap, blended with a box-filtered mip where a pixel covers more
//! than a luxel ([`crate::lightmap_mips`]). Sampling per pixel instead of
//! stepping between samples keeps a shadow edge from sliding with the span
//! grid. Bump mapping adds a normal fetch and one dot product
//! ([`crate::bump`]); specular ([`crate::specular`]) adds one more dot, a table
//! lookup and, inside a highlight, the specular mask from the alpha of the
//! normal texels. It only exists in the `SPEC = true` instantiation, so the
//! default path pays nothing for it.

use mscent_math::Vec3;
use mscent_world::lightmap::{FaceProjection, Lightmap};

use crate::bump::{FLAT_NORMAL, NormalSample, bump_gain};
use crate::face_points::FacePoints;
use crate::light_sampler::{Cell, FaceSampler, Plan};
use crate::lightmap_mips::{LightMips, Rgbx};
use crate::raster::SPAN_LEN;
use crate::shade::{Fog, SpanEnd, SpanShader, lerp_rgb};
use crate::specular::{SpecTable, half_vector};
use crate::texel_light::{add_highlight, dither_of, limit_light, mul_rgb3, spec_response};
use crate::texture::{NormalView, Texture};

pub use crate::uv_luxel::UvToLuxel;

/// Pixels of the longest span (a power of two, for the masked index).
const MAX_PX: usize = SPAN_LEN as usize;

/// Longest interval between samples of the light direction, in pixels (a
/// power of two). The direction varies slowly and costs a normalisation per
/// sample, so it is sampled sparsely and stepped linearly in between (the
/// colour is fetched per pixel).
const MAX_SEG_SHIFT: u32 = 4;
/// Shortest interval, as a shift: every sixteenth pixel (a knot costs two
/// normalisations; every eighth pixel cost about 0.4 ms a frame more).
const MIN_SEG_SHIFT: u32 = 4;
/// Intervals of the longest span (`MAX_PX >> MIN_SEG_SHIFT`).
const MAX_SEGS: usize = MAX_PX >> MIN_SEG_SHIFT;
/// Largest distance, in luxels, between two direction samples on screen.
const MAX_KNOT_LUXELS: f32 = 8.0;

/// Texels per pixel up to which bump mapping keeps full strength, and the
/// rate at which it has faded out (the normal map aliases beyond that).
const BUMP_FULL: f32 = 1.0;
const BUMP_GONE: f32 = 3.0;

/// Everything the shader needs about the face being drawn.
#[derive(Clone, Copy)]
pub struct FaceLight<'a> {
    pub lightmap: &'a Lightmap,
    /// The colour grids of `lightmap` and their pre-filtered levels, for
    /// faces that shrink below a luxel per pixel.
    pub mips: &'a LightMips,
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

/// Linear light direction and half vector over one sampling interval.
#[derive(Clone, Copy)]
struct DirSeg {
    dir0: Vec3,
    ddir: Vec3,
    half0: Vec3,
    dhalf: Vec3,
}

const EMPTY_SEG: DirSeg = DirSeg {
    dir0: FLAT_NORMAL,
    ddir: Vec3::ZERO,
    half0: FLAT_NORMAL,
    dhalf: Vec3::ZERO,
};

/// Eye position, power table and mask for the specular path.
#[derive(Clone, Copy)]
struct SpecInput<'a> {
    /// Eye in the face tangent frame (origin at the face frame's origin).
    eye_t: Vec3,
    table: &'a SpecTable,
    /// The texture's mean specular mask, for where the mask is not fetched or
    /// has faded out.
    mean_mask: f32,
}

/// Lightmap, bump and optional specular in one span shader.
pub struct LightingShader<'a, const SPEC: bool> {
    face: FaceLight<'a>,
    sampler: FaceSampler<'a>,
    plan: Plan<'a>,
    /// Level of detail `plan` was made for, and the face's luxel size in texels.
    lod: f32,
    luxels_per_texel: f32,
    /// The normal map to apply, when bump mapping is on and the texture has one.
    normals: Option<NormalView<'a>>,
    proj: FaceProjection,
    spec: Option<SpecInput<'a>>,
    /// Short-lived lights on this face; empty for nearly every face.
    points: FacePoints,
    // Per-span state: the light of each pixel, colour in 256 = full-light
    // units and directions in the texture frame.
    col: [Rgbx; MAX_PX],
    segs: [DirSeg; MAX_SEGS],
    shift: u32,
    /// The direction knot at the end of the previous span and its luxel
    /// position: the next span on the row starts exactly there.
    dir_carry: Option<(f32, f32, (Vec3, Vec3))>,
    cells: [Cell; 2],
    /// How much of the bump effect survives at the current minification.
    bump_scale: f32,
    /// Screen position of pixel 0 of the span, for the dither.
    x0: i32,
    y0: i32,
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
        let mean_mask = face.texture.spec_mean();
        let spec = SpecInput {
            eye_t,
            table,
            mean_mask,
        };
        Self::build(face, Some(spec))
    }
}

impl<'a, const SPEC: bool> LightingShader<'a, SPEC> {
    fn build(face: FaceLight<'a>, spec: Option<SpecInput<'a>>) -> Option<Self> {
        let proj = *face.lightmap.projections().get(face.face)?;
        let grid = face.lightmap.face_grid(face.face)?;
        let sampler = FaceSampler::new(grid, face.mips.levels(face.face));
        Some(Self {
            face,
            sampler,
            plan: sampler.plan(0.0),
            lod: 0.0,
            luxels_per_texel: face.map.luxels_per_texel(),
            normals: face.texture.normal_view().filter(|_| face.bump),
            proj,
            spec,
            points: FacePoints::NONE,
            col: [[0.0; 4]; MAX_PX],
            segs: [EMPTY_SEG; MAX_SEGS],
            shift: MAX_SEG_SHIFT,
            dir_carry: None,
            cells: [Cell::EMPTY; 2],
            bump_scale: 1.0,
            x0: 0,
            y0: 0,
            fog0: 0.0,
            fog_step: 0.0,
        })
    }

    /// Adds a short-lived point light (world `pos`, `color` where 1.0 is a
    /// plain texture colour at point-blank range) to this face. It joins the
    /// baked light before the albedo and fog, so a dark wall lights up. False
    /// if four lights are already on the face or a value is not usable.
    pub fn add_point(&mut self, pos: Vec3, radius: f32, color: Vec3) -> bool {
        self.points.add(&self.proj, pos, radius, color)
    }

    /// Whether anything downstream reads the baked light direction.
    fn wants_dir(&self) -> bool {
        (self.normals.is_some() && self.bump_scale > 0.0) || self.spec.is_some()
    }

    /// The specular mask of a pixel. It fades to the material's mean as the map
    /// is minified, as the normal does: the normal array has no mips, so the raw
    /// mask would shimmer. With no fetch to read it from (the fade is complete,
    /// bump mapping is off or there is no map) the mean is the answer. A mean of
    /// exactly 1 means every texel is full: no mask, nothing to blend, which
    /// keeps both the pixels and the cost of a material without one.
    #[inline]
    fn spec_mask(&self, spec: &SpecInput, sample: Option<&NormalSample>) -> f32 {
        if spec.mean_mask >= 1.0 {
            return 1.0;
        }
        match sample {
            Some(s) => spec.mean_mask + (s.mask() - spec.mean_mask) * self.bump_scale,
            None => spec.mean_mask,
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

    /// Fetch the light of every pixel of the span. Luxel coordinates are
    /// affine in the span's texture coordinates, so they are stepped from
    /// the two ends (evaluated at pixel centres, so they do not depend on
    /// where the span grid starts).
    fn begin_light(&mut self, start: &SpanEnd, end: &SpanEnd, len: u32) {
        let (ax, ay) = self.face.map.apply(start.u, start.v);
        let (bx, by) = self.face.map.apply(end.u, end.v);
        let inv = 1.0 / len.max(1) as f32;
        let (dx, dy) = ((bx - ax) * inv, (by - ay) * inv);
        let n = (len as usize).min(MAX_PX);
        self.plan.colors(
            (ax, ay),
            (dx, dy),
            256.0,
            &mut self.cells,
            &mut self.col[..n],
        );
        if !self.points.is_empty() {
            self.points
                .add_span(&mut self.col[..n], (ax, ay), (bx, by), len);
        }
        if self.wants_dir() {
            self.begin_dirs(len, (ax, ay), (bx, by));
        }
    }

    /// Sample the light direction (and half vector) along the span every
    /// `1 << shift` pixels, where the shift keeps the samples about a luxel
    /// apart, and step linearly between them.
    fn begin_dirs(&mut self, len: u32, (ax, ay): (f32, f32), (bx, by): (f32, f32)) {
        let per_px = (bx - ax).abs().max((by - ay).abs()) / len.max(1) as f32;
        self.shift = MAX_SEG_SHIFT;
        while self.shift > MIN_SEG_SHIFT && (1u32 << self.shift) as f32 * per_px > MAX_KNOT_LUXELS {
            self.shift -= 1;
        }
        let seg_len = 1u32 << self.shift;
        let t_per_px = 1.0 / len.max(1) as f32;
        let inv = 1.0 / seg_len as f32;
        // The knot at the end of the previous span is this span's start.
        let carried = self
            .dir_carry
            .filter(|&(cx, cy, _)| cx.to_bits() == ax.to_bits() && cy.to_bits() == ay.to_bits());
        let mut a = carried.map_or_else(|| self.dir_knot(ax, ay), |(_, _, k)| k);
        self.dir_carry = None;
        for k in 0..len.div_ceil(seg_len) {
            let t = ((k + 1) * seg_len) as f32 * t_per_px;
            // A span that ends on a knot samples exactly at its end, which
            // is where the next span begins.
            let at_end = (k + 1) * seg_len == len;
            let (kx, ky) = if at_end {
                (bx, by)
            } else {
                (ax + (bx - ax) * t, ay + (by - ay) * t)
            };
            let b = self.dir_knot(kx, ky);
            if at_end {
                self.dir_carry = Some((kx, ky, b));
            }
            self.segs[k as usize & (MAX_SEGS - 1)] = DirSeg {
                dir0: a.0,
                ddir: (b.0 - a.0) * inv,
                half0: a.1,
                dhalf: (b.1 - a.1) * inv,
            };
            a = b;
        }
    }

    /// Light direction and half vector, both in the texture frame, at
    /// luxel coordinates `(lx, ly)`. Bump normals live in the texture frame;
    /// the baked directions and the view vector are in the lightmap's
    /// tangent frame.
    fn dir_knot(&self, lx: f32, ly: f32) -> (Vec3, Vec3) {
        let sum = self.sampler.dir(lx, ly);
        let to_tex = |v: Vec3| self.face.map.to_texture_frame(v);
        // The bump gain is a ratio of two terms that both scale with the
        // light vector, so the blended (slightly short) vector needs no
        // normalising; the half vector does.
        let Some(spec) = self.spec else {
            return (to_tex(sum), FLAT_NORMAL);
        };
        let dir = sum.normalized().unwrap_or(FLAT_NORMAL);
        let half = half_vector(dir, self.view_dir(&spec, lx, ly));
        (to_tex(sum), to_tex(half))
    }
}

/// `log2(x)` for positive finite `x`, linear between powers of two (off by
/// under 0.09, which only shifts where the level blend happens).
fn fast_log2(x: f32) -> f32 {
    x.to_bits() as f32 * (1.0 / (1 << 23) as f32) - 127.0
}

impl<const SPEC: bool> SpanShader for LightingShader<'_, SPEC> {
    fn set_texel_rate(&mut self, texels_per_pixel: f32) {
        let t = (BUMP_GONE - texels_per_pixel) / (BUMP_GONE - BUMP_FULL);
        self.bump_scale = if t.is_nan() { 0.0 } else { t.clamp(0.0, 1.0) };
        // One luxel per pixel is the finest level; every doubling of the
        // footprint moves one level up the chain. NaN reads level 0.
        let luxels = texels_per_pixel * self.luxels_per_texel;
        // Close surfaces (under a luxel per pixel) stay on the baked grid and
        // keep their plan; only farther spans pay for a level of detail.
        let lod = if luxels > 1.0 { fast_log2(luxels) } else { 0.0 };
        if lod.to_bits() != self.lod.to_bits() {
            self.lod = lod;
            self.plan = self.sampler.plan(lod);
        }
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
        let at = i as usize & (MAX_PX - 1);
        let seg = &self.segs[(i >> self.shift) as usize & (MAX_SEGS - 1)];
        let f = (i & ((1 << self.shift) - 1)) as f32;
        let c = self.col[at];
        let mut k = Vec3::new(c[0], c[1], c[2]);
        let mut n = FLAT_NORMAL;
        // The fetch the specular path can read the mask from, if there is one.
        let mut sample = None;
        // Past BUMP_GONE the relief has faded to flat, whose gain is exactly
        // 1, so the normal fetch is skipped.
        if let Some(map) = self.normals.filter(|_| self.bump_scale > 0.0) {
            let fetched = NormalSample::fetch(&map, fu, fv);
            if SPEC {
                sample = Some(fetched);
            }
            // Fade the normal towards flat as the map is minified.
            n = fetched.normal();
            if self.bump_scale < 1.0 {
                n = FLAT_NORMAL + (n - FLAT_NORMAL) * self.bump_scale;
            }
            k = k * bump_gain(n, seg.dir0 + seg.ddir * f);
        }
        let k = limit_light(k);
        let dither = dither_of(self.x0 + i as i32, self.y0);
        let mut out = mul_rgb3(texel, k, dither);
        if let Some(spec) = self.spec.filter(|_| SPEC) {
            let h = seg.half0 + seg.dhalf * f;
            let s = spec_response(spec.table, n, h);
            out = add_highlight(out, k, s, || self.spec_mask(&spec, sample.as_ref()));
        }
        match self.face.fog {
            Some(fog) => {
                let amount = (self.fog0 + self.fog_step * i as f32).clamp(0.0, 256.0) as u32;
                // Zero fog leaves the colour exactly as it is.
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

#[cfg(test)]
#[path = "lighting_shader_tests.rs"]
mod lighting_shader_tests;
#[cfg(test)]
#[path = "specular_mask_tests.rs"]
mod specular_mask_tests;
#[cfg(test)]
#[path = "texel_light_tests.rs"]
mod texel_light_tests;
