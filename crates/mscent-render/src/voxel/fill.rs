//! Filling voxel triangles into the rows a band holds, with the look of each.

use mscent_math::Vec3;

use super::blend::fill_blended;
use super::list::{ListedChunk, SUBPIXEL, VoxelList};
use super::shader::{Plane3, VoxelShader};
use super::tri::{Corner, Gradient, area, clip_near, each_row};
use super::{LodData, TEXELS_PER_UNIT, VoxelScene, axis_map};
use crate::camera::Camera;
use crate::dynlight::DynLight;
use crate::framebuffer::Framebuffer;
use crate::raster::{Attr, DrawStats, SPAN_LEN, Span, mip_level, span_pixels};
use crate::shade::{Fog, SpanShader};
use crate::specular::SpecTable;
use crate::texture::Texture;

/// Everything a frame's triangles share.
pub(crate) struct FillContext<'a> {
    pub scene: &'a VoxelScene,
    pub cam: &'a Camera,
    pub eye: Vec3,
    pub fog: Option<Fog>,
    /// Bump mapping at this quality.
    pub bump: bool,
    /// Specular at this quality.
    pub specular: bool,
    pub lights: &'a [DynLight],
    /// One per material id, then one for unknown ids.
    pub spec: &'a [SpecTable],
    pub texture_of: &'a (dyn Fn(u16) -> &'a Texture + Sync),
}

/// Fills every listed chunk touching the rows `fb` holds.
pub(crate) fn fill_voxels(fb: &mut Framebuffer, list: &VoxelList, ctx: &FillContext) -> DrawStats {
    let mut stats = DrawStats::default();
    let band = fb.row_range();
    for listed in &list.chunks {
        if listed.first >= band.1 || listed.last <= band.0 {
            continue;
        }
        fill_chunk(fb, list, listed, ctx, band, &mut stats);
    }
    stats
}

fn fill_chunk(
    fb: &mut Framebuffer,
    list: &VoxelList,
    listed: &ListedChunk,
    ctx: &FillContext,
    band: (i32, i32),
    stats: &mut DrawStats,
) {
    let c = listed.chunk as usize;
    let lod = usize::from(listed.lod);
    let (Some(chunk), Some(data)) = (ctx.scene.world.chunks.get(c), ctx.scene.lods.get(c)) else {
        return;
    };
    let (mesh, data) = (&chunk.lods[lod], &data[lod]);
    let Some(screen) = list
        .screen
        .get(listed.start..listed.start + mesh.vertices.len())
    else {
        return;
    };
    let row16 = (band.0.saturating_mul(16), band.1.saturating_mul(16));
    for t in &mesh.triangles {
        let [ia, ib, ic] = t.map(usize::from);
        let (a, b, d) = (screen[ia], screen[ib], screen[ic]);
        let clipped = a.iz <= 0.0 || b.iz <= 0.0 || d.iz <= 0.0;
        if !clipped {
            let (lo, hi) = (a.y.min(b.y).min(d.y), a.y.max(b.y).max(d.y));
            // A row's centre is 8 sub-pixels in; this is only a cheap first cut.
            if hi < row16.0 || lo >= row16.1 {
                continue;
            }
            let cross = i64::from(b.x - a.x) * i64::from(d.y - a.y)
                - i64::from(d.x - a.x) * i64::from(b.y - a.y);
            if cross >= 0 {
                continue;
            }
        }
        let tri = Tri {
            mesh,
            data,
            index: [ia, ib, ic],
            lights: listed.lights,
            full: lod == 0,
        };
        fill_tri(fb, &tri, [a, b, d].map(|s| (s.x, s.y, s.iz)), ctx, stats);
    }
}

/// One triangle of a chunk, by its vertices.
struct Tri<'a> {
    mesh: &'a mscent_world::voxel::ChunkMesh,
    data: &'a LodData,
    index: [usize; 3],
    lights: u8,
    /// Full detail: beyond it shading is Gouraud plus albedo only.
    full: bool,
}

fn fill_tri(
    fb: &mut Framebuffer,
    tri: &Tri,
    screen: [(i32, i32, f32); 3],
    ctx: &FillContext,
    stats: &mut DrawStats,
) {
    let mut c = [Corner::default(); 3];
    for k in 0..3 {
        let i = tri.index[k];
        let v = &tri.mesh.vertices[i];
        let light = tri.data.light.get(i).copied().unwrap_or_default();
        let mut w = Vec3::ZERO;
        match k {
            0 => w.x = 1.0,
            1 => w.y = 1.0,
            _ => w.z = 1.0,
        }
        c[k] = Corner {
            x: screen[k].0,
            y: screen[k].1,
            iz: screen[k].2,
            pos: v.pos,
            light: add_dynamic(light.color, v.pos, v.normal, tri.lights, ctx.lights),
            dir: tri.data.dir_t.get(i).copied().unwrap_or(Vec3::ZERO),
            weight: w,
        };
    }
    if c.iter().all(|k| k.iz > 0.0) {
        draw_corners(fb, tri, &c, ctx, stats);
        return;
    }
    for piece in clip_near(ctx.cam, &c).into_iter().flatten() {
        draw_corners(fb, tri, &piece, ctx, stats);
    }
}

/// `color` plus the frame's dynamic lights that reach a vertex.
fn add_dynamic(color: Vec3, pos: Vec3, normal: Vec3, mask: u8, lights: &[DynLight]) -> Vec3 {
    if mask == 0 {
        return color;
    }
    let mut out = color;
    for (i, l) in lights.iter().enumerate().take(8) {
        if mask & (1 << i) == 0 {
            continue;
        }
        let to = l.pos() - pos;
        let d = to.length();
        if d >= l.radius() || d <= 0.0 {
            continue;
        }
        let fall = 1.0 - d / l.radius();
        let lambert = (normal.dot(to) / d).max(0.0);
        out = out + l.color() * (256.0 * fall * fall * lambert);
    }
    out
}

/// The same texture and axis at all three of the triangle's own vertices.
fn plain_look(tri: &Tri) -> Option<(u8, u8)> {
    let d = tri.data;
    let [a, b, c] = tri.index;
    let axis = *d.axis.get(a)?;
    let material = *d.material.get(a)?;
    (d.axis.get(b) == Some(&axis)
        && d.axis.get(c) == Some(&axis)
        && d.material.get(b) == Some(&material)
        && d.material.get(c) == Some(&material))
    .then_some((axis, material))
}

fn draw_corners(
    fb: &mut Framebuffer,
    tri: &Tri,
    c: &[Corner; 3],
    ctx: &FillContext,
    stats: &mut DrawStats,
) {
    let det = area(c);
    if det >= 0.0 {
        return;
    }
    let grad = Gradient::new(c, det);
    match plain_look(tri) {
        Some((axis, material)) => fill_plain(fb, tri, c, &grad, axis, material, ctx, stats),
        None => {
            let corners = tri.index.map(|i| {
                (
                    tri.data.axis.get(i).copied().unwrap_or(2),
                    tri.data.material.get(i).copied().unwrap_or(0),
                )
            });
            fill_blended(fb, c, &grad, corners, ctx, stats);
        }
    }
}

/// `value / depth` at each corner.
fn over_z(c: &[Corner; 3], value: impl Fn(&Corner) -> Vec3) -> [Vec3; 3] {
    [0, 1, 2].map(|k| value(&c[k]) * c[k].iz)
}

#[allow(clippy::too_many_arguments)]
fn fill_plain(
    fb: &mut Framebuffer,
    tri: &Tri,
    c: &[Corner; 3],
    grad: &Gradient,
    axis: u8,
    material: u8,
    ctx: &FillContext,
    stats: &mut DrawStats,
) {
    let id = u16::from(material);
    let tex = (ctx.texture_of)(id);
    let (u_axis, v_axis) = axis_map(axis);
    let (tw, th) = (tex.width() as f32, tex.height() as f32);
    let texel = |p: Vec3| {
        (
            p.dot(u_axis) * TEXELS_PER_UNIT,
            p.dot(v_axis) * TEXELS_PER_UNIT,
        )
    };
    let t = [texel(c[0].pos), texel(c[1].pos), texel(c[2].pos)];
    // A whole number of repeats off keeps the numbers small, and so exact.
    let (su, sv) = ((t[0].0 / tw).floor() * tw, (t[0].1 / th).floor() * th);
    let at = |f: &dyn Fn(usize) -> f32| [f(0), f(1), f(2)];
    let iz = grad.plane(at(&|k| c[k].iz));
    let uz = grad.plane(at(&|k| (t[k].0 - su) * c[k].iz));
    let vz = grad.plane(at(&|k| (t[k].1 - sv) * c[k].iz));
    let origin = Attr {
        iz: iz.0,
        uz: uz.0,
        vz: vz.0,
    };
    let gx = Attr {
        iz: iz.1,
        uz: uz.1,
        vz: vz.1,
    };
    let gy = Attr {
        iz: iz.2,
        uz: uz.2,
        vz: vz.2,
    };
    let centre = {
        let third = 1.0 / 3.0;
        let cx = (c[0].x + c[1].x + c[2].x) as f32 * third / SUBPIXEL;
        let cy = (c[0].y + c[1].y + c[2].y) as f32 * third / SUBPIXEL;
        origin.advance(gx, cx).advance(gy, cy)
    };
    let (level, rate) = mip_level(tex, centre, gx, Some(gy));
    let normals = tex.normal_view().filter(|_| ctx.bump && tri.full);
    let table = if ctx.specular && tri.full {
        ctx.spec
            .get(usize::from(id))
            .or(ctx.spec.last())
            .filter(|t| t.is_visible())
    } else {
        None
    };
    let light = grad.plane3(over_z(c, |k| k.light));
    let dir = grad.plane3(over_z(c, |k| k.dir));
    let base = Parts {
        tex,
        origin,
        gx,
        gy,
        level,
        rate,
    };
    if let Some(table) = table {
        let (u, w) = (u_axis, v_axis);
        let view = grad.plane3(over_z(c, |k| {
            let v = ctx.eye - k.pos;
            let n = tri_normal(c);
            Vec3::new(v.dot(u), -v.dot(w), v.dot(n))
        }));
        let mut shader = make_shader::<true>(light, dir, view, normals, Some(table), ctx.fog);
        fill_rows(fb, c, &base, &mut shader, stats);
    } else {
        let mut shader =
            make_shader::<false>(light, dir, Plane3::default(), normals, None, ctx.fog);
        fill_rows(fb, c, &base, &mut shader, stats);
    }
}

/// The triangle's facing, which is near enough its vertices' for the eye vector.
fn tri_normal(c: &[Corner; 3]) -> Vec3 {
    (c[1].pos - c[0].pos)
        .cross(c[2].pos - c[0].pos)
        .normalized()
        .unwrap_or(Vec3::new(0.0, 0.0, 1.0))
}

fn make_shader<'a, const SPEC: bool>(
    light: Plane3,
    dir: Plane3,
    view: Plane3,
    normals: Option<crate::texture::NormalView<'a>>,
    spec: Option<&'a SpecTable>,
    fog: Option<Fog>,
) -> VoxelShader<'a, SPEC> {
    VoxelShader {
        light,
        dir,
        view,
        normals,
        spec,
        fog,
        col0: Vec3::ZERO,
        dcol: Vec3::ZERO,
        dir0: Vec3::ZERO,
        ddir: Vec3::ZERO,
        half0: Vec3::ZERO,
        dhalf: Vec3::ZERO,
        bump_scale: 1.0,
        x0: 0,
        y0: 0,
        fog0: 0.0,
        fog_step: 0.0,
    }
}

/// What every row of a plain triangle starts from.
struct Parts<'a> {
    tex: &'a Texture,
    origin: Attr,
    gx: Attr,
    gy: Attr,
    level: u32,
    rate: f32,
}

fn fill_rows<S: SpanShader>(
    fb: &mut Framebuffer,
    c: &[Corner; 3],
    p: &Parts,
    shader: &mut S,
    stats: &mut DrawStats,
) {
    shader.set_texel_rate(p.rate);
    let band = fb.row_range();
    let width = fb.width() as i32;
    each_row(c, band, width, |y, xs, xe| {
        let Some((color, depth)) = fb.row_mut(y) else {
            return;
        };
        let mut a = p
            .origin
            .advance(p.gx, xs as f32 + 0.5)
            .advance(p.gy, y as f32 + 0.5);
        let mut x = xs;
        while x < xe {
            let n = SPAN_LEN.min(xe - x);
            let b = a.advance(p.gx, n as f32);
            let (ea, eb) = (a.at(x, y), b.at(x + n, y));
            shader.begin_span(&ea, &eb, n as u32);
            stats.spans += 1;
            let span = Span {
                x: x as usize,
                n: n as usize,
                ea,
                eb,
                iz0: a.iz,
                diz: p.gx.iz,
                level: p.level,
                smooth: p.tex.is_filtered(),
            };
            stats.pixels += span_pixels(color, depth, &span, p.tex, shader);
            a = b;
            x += n;
        }
    });
}
