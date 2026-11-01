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
struct Attr {
    iz: f32,
    uz: f32,
    vz: f32,
}

impl Attr {
    fn advance(self, g: Attr, n: f32) -> Attr {
        Attr {
            iz: self.iz + g.iz * n,
            uz: self.uz + g.uz * n,
            vz: self.vz + g.vz * n,
        }
    }

    fn at(self, x: i32, y: i32) -> SpanEnd {
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
    if verts.len() > MAX_POLYGON_VERTICES {
        return stats;
    }
    let cam_verts: Vec<ClipVertex> = verts
        .iter()
        .map(|v| ClipVertex {
            pos: cam.world_to_camera(v.pos),
            uv: v.uv,
        })
        .collect();
    let clipped = clip_polygon(&cam.frustum(), &cam_verts);
    let Some(pts) = project_all(cam, tex, &clipped) else {
        return stats;
    };
    fill_polygon(fb, &pts, tex, shader, &mut stats);
    stats
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

fn fill_polygon<S: SpanShader>(
    fb: &mut Framebuffer,
    pts: &[ScreenVert],
    tex: &Texture,
    shader: &mut S,
    stats: &mut DrawStats,
) {
    let (ymin, ymax) = pts.iter().fold((f32::MAX, f32::MIN), |(lo, hi), p| {
        (lo.min(p.y), hi.max(p.y))
    });
    let first = (ymin - 0.5).ceil().max(0.0) as i32;
    let end = ((ymax - 0.5).ceil() as i32).min(fb.height() as i32);
    for y in first..end {
        if let Some((l, r)) = row_extent(pts, y as f32 + 0.5) {
            draw_row(fb, y, (l, r), tex, shader, stats);
        }
    }
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
fn fix(f: f32) -> i32 {
    (f64::from(f) * 65536.0) as i64 as i32
}

fn draw_row<S: SpanShader>(
    fb: &mut Framebuffer,
    y: i32,
    (l, r): (ScreenVert, ScreenVert),
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
        shader.begin_span(&ea, &eb, n as u32);
        stats.spans += 1;
        let span = Span {
            x: x as usize,
            n: n as usize,
            ea,
            eb,
            iz0: a.iz,
            diz: grad.iz,
        };
        stats.pixels += span_pixels(color, depth, &span, tex, shader);
        a = b;
        x += n;
    }
}

struct Span {
    x: usize,
    n: usize,
    ea: SpanEnd,
    eb: SpanEnd,
    iz0: f32,
    diz: f32,
}

/// Inner loop: texel fetch, depth test and shade for one span. Returns the
/// number of pixels written.
fn span_pixels<S: SpanShader>(
    color: &mut [u32],
    depth: &mut [f32],
    s: &Span,
    tex: &Texture,
    shader: &S,
) -> u32 {
    let step = 1.0 / s.n as f32;
    let (mut fu, mut fv) = (fix(s.ea.u), fix(s.ea.v));
    let (du, dv) = (fix((s.eb.u - s.ea.u) * step), fix((s.eb.v - s.ea.v) * step));
    let (mx, my) = (tex.mask_x(), tex.mask_y());
    let mut written = 0;
    for i in 0..s.n {
        let iz = s.iz0 + s.diz * i as f32;
        let slot = s.x + i;
        if iz > depth[slot] {
            let (tx, ty) = ((fu >> 16) as u32 & mx, (fv >> 16) as u32 & my);
            let (tx, ty) = (tx as i32, ty as i32);
            depth[slot] = iz;
            color[slot] = shader.shade(i as u32, tex.texel(tx, ty), tx, ty);
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
