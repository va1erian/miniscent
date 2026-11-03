//! Filling voxel triangles into the rows a band holds, with the look of each.

use mscent_math::Vec3;

use super::blend::fill_blended;
use super::list::{ListedChunk, SUBPIXEL, VoxelList};
use super::shader::{Gloss, Plane3, VoxelShader};
use super::solid::has_spots;
use super::tri::{Corner, Gradient, area, clip_near, covers_a_pixel, each_row};
use super::{IN_SEAM, LodData, TEXELS_PER_UNIT, VoxelScene, axis_map};
use crate::camera::Camera;
use crate::dynlight::DynLight;
use crate::framebuffer::Framebuffer;
use crate::raster::{Attr, DrawStats, SPAN_LEN, Span, mip_level, span_pixels};
use crate::shade::{Fog, SpanShader};
use crate::specular::SpecTable;
use crate::texture::Texture;

/// Everything a frame's triangles share.
pub(crate) struct FillContext<'a> {
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
pub(crate) fn fill_voxels(
    fb: &mut Framebuffer,
    list: &VoxelList,
    scene: &VoxelScene,
    ctx: &FillContext,
) -> DrawStats {
    let mut stats = DrawStats::default();
    let band = fb.row_range();
    for listed in &list.chunks {
        if listed.first >= band.1 || listed.last <= band.0 {
            continue;
        }
        fill_chunk(fb, list, listed, scene, ctx, band, &mut stats);
    }
    stats
}

fn fill_chunk(
    fb: &mut Framebuffer,
    list: &VoxelList,
    listed: &ListedChunk,
    scene: &VoxelScene,
    ctx: &FillContext,
    band: (i32, i32),
    stats: &mut DrawStats,
) {
    let c = listed.chunk as usize;
    let lod = usize::from(listed.lod);
    let (Some(chunk), Some(data)) = (scene.world.chunks.get(c), scene.lods.get(c)) else {
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
            world_dir: light.dir,
            normal: v.normal,
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

/// The axis and material of a triangle, if it has the same of each at all three
/// of its vertices and nothing that needs the blended way.
fn plain_look(axes: &[u8], materials: &[u8], index: [usize; 3]) -> Option<(u8, u8)> {
    let [a, b, c] = index;
    let axis = axes.get(a).copied().filter(|&x| x != IN_SEAM)?;
    let material = materials.get(a).copied().filter(|&m| !has_spots(m))?;
    (axes.get(b) == Some(&axis)
        && axes.get(c) == Some(&axis)
        && materials.get(b) == Some(&material)
        && materials.get(c) == Some(&material))
    .then_some((axis, material))
}

/// Draws a triangle facing the eye and covering a pixel, by the look of the
/// vertices `index` in `axes` and `materials`.
pub(crate) fn draw_look(
    fb: &mut Framebuffer,
    c: &[Corner; 3],
    (axes, materials, index): (&[u8], &[u8], [usize; 3]),
    ctx: &FillContext,
    stats: &mut DrawStats,
) {
    let det = area(c);
    if det >= 0.0 || !covers_a_pixel(c, fb.row_range(), fb.width() as i32) {
        return;
    }
    let grad = Gradient::new(c, det);
    match plain_look(axes, materials, index) {
        Some((axis, material)) => fill_plain(fb, c, &grad, axis, material, ctx, stats),
        None => {
            let corners = index.map(|i| {
                (
                    axes.get(i).copied().unwrap_or(2),
                    materials.get(i).copied().unwrap_or(0),
                )
            });
            fill_blended(fb, c, &grad, corners, ctx, stats);
        }
    }
}

fn draw_corners(
    fb: &mut Framebuffer,
    tri: &Tri,
    c: &[Corner; 3],
    ctx: &FillContext,
    stats: &mut DrawStats,
) {
    let look = (
        tri.data.axis.as_slice(),
        tri.data.material.as_slice(),
        tri.index,
    );
    draw_look(fb, c, look, ctx, stats);
}

/// `value / depth` at each corner.
fn over_z(c: &[Corner; 3], value: impl Fn(&Corner) -> Vec3) -> [Vec3; 3] {
    [0, 1, 2].map(|k| value(&c[k]) * c[k].iz)
}

#[allow(clippy::too_many_arguments)]
fn fill_plain(
    fb: &mut Framebuffer,
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
    let normals = tex.normal_view().filter(|_| ctx.bump);
    let table = if ctx.specular {
        ctx.spec
            .get(usize::from(id))
            .or(ctx.spec.last())
            .filter(|t| t.is_visible())
    } else {
        None
    };
    let light = grad.plane3(over_z(c, |k| k.light));
    // Most far triangles have no use for it, and a plane is most of a triangle's cost.
    let dir = if normals.is_some() || table.is_some() {
        grad.plane3(over_z(c, |k| k.dir))
    } else {
        Plane3::default()
    };
    let base = Parts {
        tex,
        origin,
        gx,
        gy,
        level,
        rate,
    };
    if let Some(table) = table {
        let gloss = Gloss {
            to_light: grad.plane3(over_z(c, |k| k.world_dir)),
            pos: grad.plane3(over_z(c, |k| k.pos)),
            normal: grad.plane3(over_z(c, |k| k.normal)),
            eye: ctx.eye,
            u: u_axis,
            v: v_axis,
        };
        let mut shader = make_shader::<true>(light, dir, gloss, normals, Some(table), ctx.fog);
        fill_rows(fb, c, &base, &mut shader, stats);
    } else {
        let mut shader = make_shader::<false>(light, dir, Gloss::default(), normals, None, ctx.fog);
        fill_rows(fb, c, &base, &mut shader, stats);
    }
}

fn make_shader<'a, const SPEC: bool>(
    light: Plane3,
    dir: Plane3,
    gloss: Gloss,
    normals: Option<crate::texture::NormalView<'a>>,
    spec: Option<&'a SpecTable>,
    fog: Option<Fog>,
) -> VoxelShader<'a, SPEC> {
    VoxelShader {
        light,
        dir,
        gloss,
        normals,
        spec,
        fog,
        col0: Vec3::ZERO,
        dcol: Vec3::ZERO,
        dir0: Vec3::ZERO,
        ddir: Vec3::ZERO,
        half0: Vec3::ZERO,
        dhalf: Vec3::ZERO,
        surface0: Vec3::ZERO,
        dsurface: Vec3::ZERO,
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
            // `1/z` is linear along a span, so its nearest pixel is at an end, and
            // the z-buffer can spare a covered span all of its shading.
            let slots = &depth[x as usize..(x + n) as usize];
            let nearest = a.iz.max(a.iz + p.gx.iz * (n - 1) as f32);
            if slots.iter().all(|&d| nearest <= d) {
                a = b;
                x += n;
                continue;
            }
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
