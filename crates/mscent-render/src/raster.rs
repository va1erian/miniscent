//! Span rasterizer for convex textured polygons.
//!
//! Pipeline: world -> camera space, frustum clip, perspective projection,
//! then a scanline fill. `1/z`, `u/z` and `v/z` are linear in screen space, so
//! they are interpolated along edges and rows; the true `u`, `v` are
//! recovered with one divide per 16-pixel span end and stepped linearly in
//! between (perspective-correct to within the span, the classic software
//! renderer compromise).
//!
//! Coverage follows the usual top-left style rule by sampling at pixel
//! centres with half-open intervals, so two polygons sharing an edge cover
//! every pixel exactly once. Triangle winding is ignored (no back-face
//! culling): callers that want it cull before drawing.

use mscent_math::{Vec2, Vec3};

use crate::camera::Camera;
use crate::clip::{ClipVertex, MAX_POLYGON_VERTICES, clip_polygon};
use crate::framebuffer::Framebuffer;
use crate::raster_masked::masked_span_pixels;
use crate::shade::{FlatShader, Fog, SpanEnd, SpanShader};
use crate::texture::Texture;

/// Pixels per perspective-correct span.
pub const SPAN_LEN: i32 = 16;

/// A world-space polygon vertex. `uv` is in texture repeats: 1.0 is one full
/// trip across the texture, and values wrap.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Vertex {
    pub pos: Vec3,
    pub uv: Vec2,
}

impl Vertex {
    pub fn new(pos: Vec3, u: f32, v: f32) -> Self {
        Self {
            pos,
            uv: Vec2::new(u, v),
        }
    }
}

/// What a draw call did, for tests and profiling.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DrawStats {
    /// Pixels that passed the depth test and were written.
    pub pixels: u32,
    /// Spans handed to the shader.
    pub spans: u32,
}

/// A projected vertex: screen position plus the screen-linear attributes.
#[derive(Clone, Copy, Debug)]
struct ScreenVert {
    x: f32,
    y: f32,
    iz: f32,
    uz: f32,
    vz: f32,
}

impl ScreenVert {
    fn lerp(self, o: Self, t: f32) -> Self {
        let l = |a: f32, b: f32| a + (b - a) * t;
        Self {
            x: l(self.x, o.x),
            y: l(self.y, o.y),
            iz: l(self.iz, o.iz),
            uz: l(self.uz, o.uz),
            vz: l(self.vz, o.vz),
        }
    }
}

/// Screen-linear attributes along a row.
#[derive(Clone, Copy)]
pub(crate) struct Attr {
    pub iz: f32,
    pub uz: f32,
    pub vz: f32,
}

impl Attr {
    pub(crate) fn advance(self, g: Attr, n: f32) -> Attr {
        Attr {
            iz: self.iz + g.iz * n,
            uz: self.uz + g.uz * n,
            vz: self.vz + g.vz * n,
        }
    }

    pub(crate) fn at(self, x: i32, y: i32) -> SpanEnd {
        // Interpolated 1/z stays positive inside a clipped polygon; the floor
        // only guards against rounding at the near plane.
        let iz = self.iz.max(1e-12);
        SpanEnd {
            x,
            y,
            u: self.uz / iz,
            v: self.vz / iz,
            depth: 1.0 / iz,
        }
    }
}

/// Draw a convex polygon with `shader`. The depth test and the output both
/// use the framebuffer's size; the camera only fixes the projection.
pub fn draw_polygon<S: SpanShader>(
    fb: &mut Framebuffer,
    cam: &Camera,
    verts: &[Vertex],
    tex: &Texture,
    shader: &mut S,
) -> DrawStats {
    let mut stats = DrawStats::default();
    if let Some(pts) = clip_and_project(cam, verts, tex) {
        fill_polygon(fb, &pts, tex, shader, &mut stats);
    }
    stats
}

/// The screen polygon of `verts`: camera space, frustum clip, projection.
/// `None` when it has too many vertices or cannot be projected.
fn clip_and_project(cam: &Camera, verts: &[Vertex], tex: &Texture) -> Option<Vec<ScreenVert>> {
    if verts.len() > MAX_POLYGON_VERTICES {
        return None;
    }
    let cam_verts: Vec<ClipVertex> = verts
        .iter()
        .map(|v| ClipVertex {
            pos: cam.world_to_camera(v.pos),
            uv: v.uv,
        })
        .collect();
    let clipped = clip_polygon(&cam.frustum(), &cam_verts);
    project_all(cam, tex, &clipped)
}

/// Draw with a constant light (256 = full) and optional fog: the flat-lit
/// fallback for faces without a lightmap.
pub fn draw_polygon_flat(
    fb: &mut Framebuffer,
    cam: &Camera,
    verts: &[Vertex],
    tex: &Texture,
    light: u32,
    fog: Option<Fog>,
) -> DrawStats {
    draw_polygon(fb, cam, verts, tex, &mut FlatShader::new(light, fog))
}

fn project_all(cam: &Camera, tex: &Texture, clipped: &[ClipVertex]) -> Option<Vec<ScreenVert>> {
    let (tw, th) = (tex.width() as f32, tex.height() as f32);
    clipped
        .iter()
        .map(|c| {
            let p = cam.project(c.pos)?;
            Some(ScreenVert {
                x: p.x,
                y: p.y,
                iz: p.inv_z,
                uz: c.uv.x * tw * p.inv_z,
                vz: c.uv.y * th * p.inv_z,
            })
        })
        .collect()
}

/// Polygons clipped and projected once, to be filled by every band of a frame:
/// a band then pays only for the polygons in its rows.
#[derive(Clone, Debug, Default)]
pub(crate) struct PolyList {
    pts: Vec<ScreenVert>,
    polys: Vec<ListedPoly>,
}

/// One polygon of a [`PolyList`]. `tag` says which polygon this was, for the
/// owner of the list to find its shader.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ListedPoly {
    pub tag: u32,
    start: usize,
    end: usize,
    /// Screen rows `first..last` it may cover.
    first: i32,
    last: i32,
}

impl PolyList {
    pub(crate) fn clear(&mut self) {
        self.pts.clear();
        self.polys.clear();
    }

    /// Clips and projects a polygon; one that cannot be seen is not listed.
    pub(crate) fn push(&mut self, cam: &Camera, verts: &[Vertex], tex: &Texture, tag: u32) {
        let Some(pts) = clip_and_project(cam, verts, tex) else {
            return;
        };
        let (first, last) = row_span(&pts);
        if first < last {
            let start = self.pts.len();
            self.pts.extend_from_slice(&pts);
            self.polys.push(ListedPoly {
                tag,
                start,
                end: self.pts.len(),
                first,
                last,
            });
        }
    }

    /// Screen bounds `[x0, y0, x1, y1]` of the polygons tagged `tag`.
    pub(crate) fn bounds_of(&self, tag: u32) -> Option<[f32; 4]> {
        let mut bounds = None;
        for poly in self.polys.iter().filter(|p| p.tag == tag) {
            for p in self.pts.get(poly.start..poly.end).unwrap_or(&[]) {
                let [x0, y0, x1, y1] = bounds.unwrap_or([p.x, p.y, p.x, p.y]);
                bounds = Some([x0.min(p.x), y0.min(p.y), x1.max(p.x), y1.max(p.y)]);
            }
        }
        bounds
    }

    /// The polygons touching rows `lo..hi`, in the order they were pushed.
    pub(crate) fn in_rows(&self, (lo, hi): (i32, i32)) -> impl Iterator<Item = ListedPoly> + '_ {
        self.polys
            .iter()
            .copied()
            .filter(move |p| p.first < hi && p.last > lo)
    }

    /// Fills `poly`, one of this list's, into the rows `fb` holds.
    pub(crate) fn fill<S: SpanShader>(
        &self,
        fb: &mut Framebuffer,
        poly: ListedPoly,
        tex: &Texture,
        shader: &mut S,
    ) -> DrawStats {
        let mut stats = DrawStats::default();
        if let Some(pts) = self.pts.get(poly.start..poly.end) {
            fill_polygon(fb, pts, tex, shader, &mut stats);
        }
        stats
    }
}

/// Screen rows `first..last` whose centres a polygon can cover.
fn row_span(pts: &[ScreenVert]) -> (i32, i32) {
    let (ymin, ymax) = pts.iter().fold((f32::MAX, f32::MIN), |(lo, hi), p| {
        (lo.min(p.y), hi.max(p.y))
    });
    (
        (ymin - 0.5).ceil().max(0.0) as i32,
        (ymax - 0.5).ceil() as i32,
    )
}

fn fill_polygon<S: SpanShader>(
    fb: &mut Framebuffer,
    pts: &[ScreenVert],
    tex: &Texture,
    shader: &mut S,
    stats: &mut DrawStats,
) {
    // A tile only holds some rows; clipping here keeps rows outside it free.
    let (row_lo, row_hi) = fb.row_range();
    let (first, last) = row_span(pts);
    let first = first.max(row_lo);
    let end = last.min(row_hi);
    let gy = vertical_gradient(pts);
    for y in first..end {
        if let Some((l, r)) = row_extent(pts, y as f32 + 0.5) {
            draw_row(fb, y, (l, r), gy, tex, shader, stats);
        }
    }
}

/// Change of the screen-linear attributes per pixel row, from the plane
/// through the three most spread-out vertices. `None` for a degenerate polygon.
fn vertical_gradient(pts: &[ScreenVert]) -> Option<Attr> {
    let p0 = *pts.first()?;
    let cross =
        |a: &ScreenVert, b: &ScreenVert| (a.x - p0.x) * (b.y - p0.y) - (b.x - p0.x) * (a.y - p0.y);
    let (mut best, mut pair) = (0.0f32, None);
    for (i, a) in pts.iter().enumerate() {
        for b in pts.iter().skip(i + 1) {
            let area = cross(a, b).abs();
            if area > best {
                (best, pair) = (area, Some((*a, *b)));
            }
        }
    }
    let (p1, p2) = pair?;
    let det = cross(&p1, &p2);
    let ay =
        |a0: f32, a1: f32, a2: f32| ((a2 - a0) * (p1.x - p0.x) - (a1 - a0) * (p2.x - p0.x)) / det;
    Some(Attr {
        iz: ay(p0.iz, p1.iz, p2.iz),
        uz: ay(p0.uz, p1.uz, p2.uz),
        vz: ay(p0.vz, p1.vz, p2.vz),
    })
}

/// Texels per pixel at a span centre, and the mip level that follows from it:
/// the largest texel step per pixel along x or y, with level 0 kept up to 1.5
/// texels per pixel so nearby surfaces stay sharp.
pub(crate) fn mip_level(tex: &Texture, centre: Attr, gx: Attr, gy: Option<Attr>) -> (u32, f32) {
    let inv = 1.0 / centre.iz.max(1e-12);
    let (u, v) = (centre.uz * inv, centre.vz * inv);
    let step = |g: Attr| {
        let du = (g.uz - u * g.iz) * inv;
        let dv = (g.vz - v * g.iz) * inv;
        du.abs().max(dv.abs())
    };
    // Cheap anisotropy: the long axis may be up to 4x blurrier than the
    // short one, so grazing floors keep detail instead of smearing.
    let (a, b) = (step(gx), gy.map_or(0.0, step));
    let rate = a.max(b).min(a.min(b) * 4.0).max(a.max(b) * 0.25);
    if tex.mip_count() == 0 || rate.is_nan() || rate <= 1.5 {
        return (0, rate);
    }
    (
        ((rate / 1.5).log2().ceil() as u32).min(tex.mip_count()),
        rate,
    )
}

/// Left and right edge crossings of a convex polygon at scanline `yc`.
fn row_extent(pts: &[ScreenVert], yc: f32) -> Option<(ScreenVert, ScreenVert)> {
    let mut lo: Option<ScreenVert> = None;
    let mut hi: Option<ScreenVert> = None;
    let mut prev = *pts.last()?;
    for &cur in pts {
        // Half-open in y so a vertex shared by two edges counts once.
        if (prev.y <= yc) != (cur.y <= yc) {
            let s = prev.lerp(cur, (yc - prev.y) / (cur.y - prev.y));
            if lo.is_none_or(|l| s.x < l.x) {
                lo = Some(s);
            }
            if hi.is_none_or(|h| s.x > h.x) {
                hi = Some(s);
            }
        }
        prev = cur;
    }
    Some((lo?, hi?))
}

/// 16.16 fixed point; the wrap to 32 bits is intended because texture sides
/// are powers of two no larger than 4096, which divide 2^16 * 2^16.
pub(crate) fn fix(f: f32) -> i32 {
    (f64::from(f) * 65536.0) as i64 as i32
}

fn draw_row<S: SpanShader>(
    fb: &mut Framebuffer,
    y: i32,
    (l, r): (ScreenVert, ScreenVert),
    gy: Option<Attr>,
    tex: &Texture,
    shader: &mut S,
    stats: &mut DrawStats,
) {
    let width = fb.width() as i32;
    let xs = (l.x - 0.5).ceil().max(0.0) as i32;
    let xe = ((r.x - 0.5).ceil() as i32).min(width);
    if xs >= xe {
        return;
    }
    let Some((color, depth)) = fb.row_mut(y) else {
        return;
    };
    let inv = 1.0 / (r.x - l.x);
    let grad = Attr {
        iz: (r.iz - l.iz) * inv,
        uz: (r.uz - l.uz) * inv,
        vz: (r.vz - l.vz) * inv,
    };
    let start = Attr {
        iz: l.iz,
        uz: l.uz,
        vz: l.vz,
    }
    .advance(grad, xs as f32 + 0.5 - l.x);
    let mut x = xs;
    let mut a = start;
    while x < xe {
        let n = SPAN_LEN.min(xe - x);
        let b = a.advance(grad, n as f32);
        let (ea, eb) = (a.at(x, y), b.at(x + n, y));
        let (level, rate) = mip_level(tex, a.advance(grad, n as f32 * 0.5), grad, gy);
        shader.set_texel_rate(rate);
        shader.begin_span(&ea, &eb, n as u32);
        stats.spans += 1;
        let span = Span {
            x: x as usize,
            n: n as usize,
            ea,
            eb,
            iz0: a.iz,
            diz: grad.iz,
            level,
            smooth: tex.is_filtered(),
        };
        // One branch per span picks the cutout loop; opaque textures never see it.
        stats.pixels += if tex.is_masked() {
            masked_span_pixels(color, depth, &span, tex, shader)
        } else {
            span_pixels(color, depth, &span, tex, shader)
        };
        a = b;
        x += n;
    }
}

pub(crate) struct Span {
    pub x: usize,
    pub n: usize,
    pub ea: SpanEnd,
    pub eb: SpanEnd,
    pub iz0: f32,
    pub diz: f32,
    pub level: u32,
    /// Filtered texture: sample bilinearly, from the span's mip level.
    pub smooth: bool,
}

/// Inner loop: texel fetch, depth test and shade for one span. Returns the
/// number of pixels written.
pub(crate) fn span_pixels<S: SpanShader>(
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
    let mut written = 0;
    for i in 0..s.n {
        let iz = s.iz0 + s.diz * i as f32;
        let slot = s.x + i;
        if iz > depth[slot] {
            depth[slot] = iz;
            let texel = if s.smooth {
                lod.bilinear(fu, fv)
            } else {
                lod.nearest(fu >> 16, fv >> 16)
            };
            color[slot] = shader.shade(i as u32, texel, fu, fv);
            written += 1;
        }
        fu = fu.wrapping_add(du);
        fv = fv.wrapping_add(dv);
    }
    written
}

#[cfg(test)]
#[path = "raster_tests.rs"]
mod raster_tests;
