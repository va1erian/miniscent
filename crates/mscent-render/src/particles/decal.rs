//! Scorch decals: a convex polygon on a wall that darkens what is under it.
//!
//! A decal is a few points with screen position, `1/z` and a position in a
//! mask. All three of `1/z`, `s/z` and `t/z` are linear in screen space, so
//! each is a plane through the points; a pixel evaluates those planes
//! directly and divides once to get the mask position. Nothing depends on
//! the row a band starts at, so a band and a whole frame give the same pixels.
//!
//! The decal lies in its wall's plane, so its `1/z` equals the stored one up
//! to rounding. The test lets it be [`DECAL_DEPTH_BIAS`] nearer than the
//! scene in relative terms and never writes depth, so it neither fights the
//! wall nor hides what is in front of it.

use mscent_math::Vec3;
use mscent_math::epsilon::DECAL_DEPTH_BIAS;

use super::Mask;
use super::bank::SpriteBank;
use super::sprite::mip_for;
use crate::camera::Camera;
use crate::framebuffer::Framebuffer;

/// Most points of a decal; a convex polygon clipped by the near plane gains
/// one, so a world polygon may have one fewer.
pub const MAX_DECAL_POINTS: usize = 16;
/// Points further out than this are refused, which keeps every difference of
/// coordinates finite.
const COORD_LIMIT: f32 = 1.0e6;
const OPAQUE: u32 = 256;
/// Pixels between exact mask positions.
const SPAN: usize = 16;

/// A world-space corner of a decal. `s` and `t` (0 to 1) place it in the mask.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DecalVertex {
    pub pos: Vec3,
    pub s: f32,
    pub t: f32,
}

/// A projected corner.
#[derive(Clone, Copy, Debug, Default)]
pub struct DecalPoint {
    pub x: f32,
    pub y: f32,
    pub inv_z: f32,
    pub s: f32,
    pub t: f32,
}

/// A decal ready to draw: `len` points in order around a convex polygon.
#[derive(Clone, Copy, Debug)]
pub struct DecalDraw {
    pub points: [DecalPoint; MAX_DECAL_POINTS],
    pub len: u8,
    /// How much the centre of the mask darkens, `0..=256` (more is clamped).
    pub darkness: u16,
    pub mask: Mask,
}

impl DecalDraw {
    fn points(&self) -> &[DecalPoint] {
        &self.points[..usize::from(self.len).min(MAX_DECAL_POINTS)]
    }

    /// Pixels on a `w` by `h` screen, for the budget: the polygon's area, cut
    /// by the share of its box that is on the screen. Zero if not finite.
    pub fn screen_area(&self, w: f32, h: f32) -> f32 {
        let pts = self.points();
        let twice = twice_area(pts);
        let (mut lo, mut hi) = ((f32::MAX, f32::MAX), (f32::MIN, f32::MIN));
        for p in pts {
            lo = (lo.0.min(p.x), lo.1.min(p.y));
            hi = (hi.0.max(p.x), hi.1.max(p.y));
        }
        let boxed = (hi.0 - lo.0) * (hi.1 - lo.1);
        let seen = (hi.0.min(w) - lo.0.max(0.0)).max(0.0) * (hi.1.min(h) - lo.1.max(0.0)).max(0.0);
        let area = twice.abs() * 0.5 * seen / boxed;
        if area.is_finite() { area } else { 0.0 }
    }
}

/// Twice the signed area of a screen polygon (y down), by the shoelace sum.
fn twice_area(pts: &[DecalPoint]) -> f32 {
    let Some(&last) = pts.last() else { return 0.0 };
    let mut prev = last;
    let mut twice = 0.0;
    for &p in pts {
        twice += prev.x * p.y - p.x * prev.y;
        prev = p;
    }
    twice
}

/// Near-clip a world polygon, project it and build the draw. The points go
/// counter-clockwise seen from the front of the wall; a decal seen from
/// behind is dropped, which saves charging for marks on the far side of a
/// wall. `None` too when it has too few or too many points, any value is not
/// finite, it is wholly behind the near plane, or it is wholly off the screen.
pub fn decal_from_world(
    cam: &Camera,
    verts: &[DecalVertex],
    darkness: u16,
    mask: Mask,
) -> Option<DecalDraw> {
    if !(3..MAX_DECAL_POINTS).contains(&verts.len()) {
        return None;
    }
    let mut cam_verts = [(Vec3::ZERO, 0.0, 0.0); MAX_DECAL_POINTS];
    for (slot, v) in cam_verts.iter_mut().zip(verts) {
        let c = cam.world_to_camera(v.pos);
        if !(c.is_finite() && v.s.is_finite() && v.t.is_finite()) {
            return None;
        }
        *slot = (c, v.s, v.t);
    }
    let mut clipped = [(Vec3::ZERO, 0.0, 0.0); MAX_DECAL_POINTS];
    let n = clip_near(cam.near(), &cam_verts[..verts.len()], &mut clipped);
    let mut points = [DecalPoint::default(); MAX_DECAL_POINTS];
    for (p, &(c, s, t)) in points.iter_mut().zip(&clipped[..n]) {
        let q = cam.project(c)?;
        *p = DecalPoint {
            x: q.x,
            y: q.y,
            inv_z: q.inv_z,
            s,
            t,
        };
    }
    let draw = DecalDraw {
        points,
        len: n as u8,
        darkness,
        mask,
    };
    on_screen(&draw, cam.width() as f32, cam.height() as f32).then_some(draw)
}

/// Keep the part of a convex polygon at or beyond the near plane; at most one
/// point more than it had. Returns how many points were written.
fn clip_near(
    near: f32,
    input: &[(Vec3, f32, f32)],
    out: &mut [(Vec3, f32, f32); MAX_DECAL_POINTS],
) -> usize {
    let Some(&last) = input.last() else { return 0 };
    let (mut prev, mut prev_d) = (last, -last.0.z - near);
    let mut n = 0;
    for &cur in input {
        let cur_d = -cur.0.z - near;
        if (cur_d >= 0.0) != (prev_d >= 0.0) && n < MAX_DECAL_POINTS {
            let k = prev_d / (prev_d - cur_d);
            let lerp = |a: f32, b: f32| a + (b - a) * k;
            out[n] = (
                prev.0.lerp(cur.0, k),
                lerp(prev.1, cur.1),
                lerp(prev.2, cur.2),
            );
            n += 1;
        }
        if cur_d >= 0.0 && n < MAX_DECAL_POINTS {
            out[n] = cur;
            n += 1;
        }
        (prev, prev_d) = (cur, cur_d);
    }
    n
}

fn on_screen(d: &DecalDraw, w: f32, h: f32) -> bool {
    let pts = d.points();
    // Counter-clockwise in the world is clockwise on a y-down screen, which
    // makes the shoelace sum negative. A NaN sum fails too.
    pts.len() >= 3
        && twice_area(pts) < 0.0
        && !pts.iter().all(|p| p.x < 0.0)
        && !pts.iter().all(|p| p.x > w)
        && !pts.iter().all(|p| p.y < 0.0)
        && !pts.iter().all(|p| p.y > h)
}

/// Screen-linear attributes as a plane: `value = base + gx * dx + gy * dy`
/// from the first point.
pub(super) struct Planes {
    pub origin: (f32, f32),
    pub base: [f32; 3],
    pub gx: [f32; 3],
    pub gy: [f32; 3],
}

impl Planes {
    /// Through the first point and the two that span the most area. `None`
    /// for a polygon with no area.
    fn fit(pts: &[DecalPoint]) -> Option<Self> {
        let p0 = *pts.first()?;
        let attrs = |p: &DecalPoint| [p.inv_z, p.s * p.inv_z, p.t * p.inv_z];
        let rel = |p: &DecalPoint| (p.x - p0.x, p.y - p0.y);
        let cross = |a: (f32, f32), b: (f32, f32)| a.0 * b.1 - b.0 * a.1;
        let (mut best, mut pair) = (0.0f32, None);
        for (i, a) in pts.iter().enumerate() {
            for b in &pts[i + 1..] {
                let area = cross(rel(a), rel(b)).abs();
                if area > best {
                    (best, pair) = (area, Some((*a, *b)));
                }
            }
        }
        let (p1, p2) = pair?;
        let (r1, r2) = (rel(&p1), rel(&p2));
        let det = cross(r1, r2);
        let (a0, a1, a2) = (attrs(&p0), attrs(&p1), attrs(&p2));
        let mut planes = Self {
            origin: (p0.x, p0.y),
            base: a0,
            gx: [0.0; 3],
            gy: [0.0; 3],
        };
        for k in 0..3 {
            let (d1, d2) = (a1[k] - a0[k], a2[k] - a0[k]);
            planes.gx[k] = (d1 * r2.1 - d2 * r1.1) / det;
            planes.gy[k] = (r1.0 * d2 - r2.0 * d1) / det;
        }
        let finite = planes.gx.iter().chain(&planes.gy).all(|g| g.is_finite());
        finite.then_some(planes)
    }

    fn at(&self, x: f32, y: f32) -> [f32; 3] {
        let (dx, dy) = (x - self.origin.0, y - self.origin.1);
        core::array::from_fn(|k| self.base[k] + self.gx[k] * dx + self.gy[k] * dy)
    }

    /// Just `1/z`, which the depth test needs for every pixel.
    fn inv_z(&self, x: f32, y: f32) -> f32 {
        self.base[0] + self.gx[0] * (x - self.origin.0) + self.gy[0] * (y - self.origin.1)
    }
}

/// How a decal reads its mask: its planes, where its mip starts in the bank's
/// bytes and its side, and how hard it darkens (`0..=256`).
pub(super) struct Layout {
    pub planes: Planes,
    pub start: usize,
    pub side: usize,
    pub darkness: u32,
}

/// The layout of `d`. `None` for a decal with bad values, which draws nothing.
pub(super) fn layout(d: &DecalDraw) -> Option<Layout> {
    let pts = d.points();
    let sane = |p: &DecalPoint| {
        let near = |v: f32| v.abs() <= COORD_LIMIT;
        let unit = |v: f32| v.is_finite();
        near(p.x) && near(p.y) && p.inv_z.is_finite() && p.inv_z > 0.0 && unit(p.s) && unit(p.t)
    };
    if pts.len() < 3 || !pts.iter().all(sane) {
        return None;
    }
    let planes = Planes::fit(pts)?;
    let extent = pts.iter().fold(0.0f32, |m, p| {
        m.max((p.x - planes.origin.0).abs())
            .max((p.y - planes.origin.1).abs())
    });
    let (start, side) = SpriteBank::mip_at(d.mask, mip_for(2.0 * extent));
    Some(Layout {
        planes,
        start,
        side,
        darkness: u32::from(d.darkness).min(OPAQUE),
    })
}

/// Darken the pixels of `d` that lie in the rows `fb` holds and are not
/// behind the scene. A decal with bad values draws nothing.
pub(super) fn draw_decal(fb: &mut Framebuffer, d: &DecalDraw, bank: &SpriteBank) {
    let pts = d.points();
    let Some(Layout {
        planes,
        start,
        side,
        darkness,
    }) = layout(d)
    else {
        return;
    };
    let (rows0, rows1) = fb.row_range();
    let (ymin, ymax) = pts.iter().fold((f32::MAX, f32::MIN), |(lo, hi), p| {
        (lo.min(p.y), hi.max(p.y))
    });
    let edge = |v: f32| (v - 0.5).ceil().clamp(rows0 as f32, rows1 as f32) as i32;
    let Some(mask) = bank.bytes().get(start..start + side * side) else {
        return;
    };
    let tex = Texels {
        mask,
        side,
        darkness,
    };
    let width = fb.width() as usize;
    for y in edge(ymin)..edge(ymax) {
        let Some((lo, hi)) = row_extent(pts, y as f32 + 0.5) else {
            continue;
        };
        let to_px = |v: f32| (v - 0.5).ceil().clamp(0.0, width as f32) as usize;
        let (xs, xe) = (to_px(lo), to_px(hi));
        let Some((color, depth)) = fb.row_mut(y) else {
            continue;
        };
        if xs >= xe {
            continue;
        }
        let row = Row {
            x0: xs,
            py: y as f32 + 0.5,
        };
        shade_row(&mut color[xs..xe], &depth[xs..xe], &row, &planes, &tex);
    }
}

/// The mask a decal reads and how hard it darkens.
struct Texels<'a> {
    mask: &'a [u8],
    side: usize,
    darkness: u32,
}

/// Where a run of pixels lies: its first column and its row's centre.
struct Row {
    x0: usize,
    py: f32,
}

/// Darken one run of pixels of a row. The mask position is exact at both ends
/// of each [`SPAN`] and stepped linearly between, which costs one divide per
/// end instead of one per pixel. A run starts at the polygon's edge on its
/// row, so a band and a whole frame step from the same places.
fn shade_row(color: &mut [u32], depth: &[f32], row: &Row, planes: &Planes, tex: &Texels) {
    let side = tex.side as f32;
    let at = |x: usize| {
        let [iz, sz, tz] = planes.at((row.x0 + x) as f32 + 0.5, row.py);
        let scale = side / iz;
        (sz * scale, tz * scale)
    };
    let len = color.len().min(depth.len());
    for start in (0..len).step_by(SPAN) {
        let end = (start + SPAN).min(len);
        let (a, b) = (at(start), at(end - 1));
        let step = 1.0 / (end - start).saturating_sub(1).max(1) as f32;
        let (ds, dt) = ((b.0 - a.0) * step, (b.1 - a.1) * step);
        let pixels = color[start..end].iter_mut().zip(&depth[start..end]);
        for (i, (c, &z)) in pixels.enumerate() {
            let iz = planes.inv_z((row.x0 + start + i) as f32 + 0.5, row.py);
            // `z > 0` leaves pixels the scene did not draw alone; a NaN fails.
            if !(z > 0.0 && iz * (1.0 + DECAL_DEPTH_BIAS) >= z) {
                continue;
            }
            let k = i as f32;
            // Casts saturate, so a rounding error past an edge lands on it.
            let last = tex.side as i32 - 1;
            let u = ((a.0 + ds * k) as i32).clamp(0, last) as usize;
            let v = ((a.1 + dt * k) as i32).clamp(0, last) as usize;
            let m = u32::from(tex.mask.get(v * tex.side + u).copied().unwrap_or(0));
            let keep = OPAQUE - ((m * tex.darkness) >> 8);
            if keep < OPAQUE {
                *c = darken(*c, keep);
            }
        }
    }
}

/// `c` with its colour channels scaled by `keep / 256` (at most 256), alpha
/// opaque: two channels at a time, which cannot carry into the next lane
/// because a product is at most 255 * 256.
fn darken(c: u32, keep: u32) -> u32 {
    let rb = (((c & 0x00ff_00ff) * keep) >> 8) & 0x00ff_00ff;
    let g = (((c & 0x0000_ff00) * keep) >> 8) & 0x0000_ff00;
    0xff00_0000 | rb | g
}

/// Left and right crossings of a convex polygon with scanline `yc`.
fn row_extent(pts: &[DecalPoint], yc: f32) -> Option<(f32, f32)> {
    let mut span: Option<(f32, f32)> = None;
    let mut prev = *pts.last()?;
    for &cur in pts {
        // Half-open in y so a point shared by two edges counts once.
        if (prev.y <= yc) != (cur.y <= yc) {
            let x = prev.x + (cur.x - prev.x) * ((yc - prev.y) / (cur.y - prev.y));
            span = Some(span.map_or((x, x), |(lo, hi)| (lo.min(x), hi.max(x))));
        }
        prev = cur;
    }
    span
}

#[cfg(test)]
#[path = "decal_tests.rs"]
mod decal_tests;
