//! The geometry of one voxel triangle: where its pixels are.
//!
//! Coverage cannot leak or double-draw. Vertices are snapped to 1/16 pixel, and a
//! triangle takes the rows and pixels whose centres fall in half-open intervals
//! between its edges. An edge's crossing of a row is computed from its top
//! endpoint alone, so the two triangles sharing an edge get the same number, bit
//! for bit, and every pixel centre lands on exactly one side of it.
//!
//! The near plane cuts a shared edge from its inside end, for the same reason:
//! both triangles then make the same new vertex.

use mscent_math::Vec3;

use super::list::{SUBPIXEL, snap};
use super::shader::Plane3;
use crate::camera::Camera;

/// A triangle corner with everything that is interpolated across it.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct Corner {
    pub x: i32,
    pub y: i32,
    pub iz: f32,
    pub pos: Vec3,
    pub light: Vec3,
    pub dir: Vec3,
    /// The light direction in the world, and the surface's normal, for a triangle
    /// whose corners have no one tangent frame.
    pub world_dir: Vec3,
    pub normal: Vec3,
    /// Weights of the triangle's three original vertices.
    pub weight: Vec3,
}

/// Change of a screen-linear value per pixel, and its value at the screen's
/// origin, so the arithmetic of a far-off vertex stays out of the rows drawn.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct Gradient {
    x0: f64,
    y0: f64,
    /// `y1 - y0` and so on, in pixels.
    e: [f64; 4],
    /// One over the determinant: the one division a triangle needs.
    inv_det: f64,
}

impl Gradient {
    pub(super) fn new(c: &[Corner; 3], det: f64) -> Self {
        let px = |v: i32| f64::from(v) / f64::from(SUBPIXEL);
        Self {
            x0: px(c[0].x),
            y0: px(c[0].y),
            e: [
                px(c[1].x - c[0].x),
                px(c[1].y - c[0].y),
                px(c[2].x - c[0].x),
                px(c[2].y - c[0].y),
            ],
            inv_det: 1.0 / det,
        }
    }

    /// `(value at the screen origin, per x, per y)`.
    pub(super) fn plane(&self, v: [f32; 3]) -> (f32, f32, f32) {
        let (d1, d2) = (f64::from(v[1] - v[0]), f64::from(v[2] - v[0]));
        let ax = (d1 * self.e[3] - d2 * self.e[1]) * self.inv_det;
        let ay = (d2 * self.e[0] - d1 * self.e[2]) * self.inv_det;
        let at = f64::from(v[0]) - ax * self.x0 - ay * self.y0;
        (at as f32, ax as f32, ay as f32)
    }

    pub(super) fn plane3(&self, v: [Vec3; 3]) -> Plane3 {
        let (ax, bx, cx) = self.plane([v[0].x, v[1].x, v[2].x]);
        let (ay, by, cy) = self.plane([v[0].y, v[1].y, v[2].y]);
        let (az, bz, cz) = self.plane([v[0].z, v[1].z, v[2].z]);
        Plane3 {
            at: Vec3::new(ax, ay, az),
            dx: Vec3::new(bx, by, bz),
            dy: Vec3::new(cx, cy, cz),
        }
    }
}

/// The rows of a triangle: for each, the half-open pixel range it covers.
pub(super) struct Rows {
    v: [(f64, f64); 3],
    slope: [f64; 3],
}

/// Top, middle and bottom, by sub-pixel row. Ties do not matter: an edge with no
/// height is never asked for a crossing.
fn sort_rows(c: &[Corner; 3]) -> [(i32, i32); 3] {
    let mut p = [(c[0].y, c[0].x), (c[1].y, c[1].x), (c[2].y, c[2].x)];
    p.sort_unstable();
    p.map(|(y, x)| (x, y))
}

impl Rows {
    fn new(c: &[Corner; 3]) -> Self {
        let p = sort_rows(c).map(|(x, y)| (f64::from(x), f64::from(y)));
        let slope = |a: (f64, f64), b: (f64, f64)| (b.0 - a.0) / (b.1 - a.1);
        Self {
            v: p,
            slope: [slope(p[0], p[2]), slope(p[0], p[1]), slope(p[1], p[2])],
        }
    }

    /// Screen rows `first..end` whose centres the triangle holds.
    fn range(&self, band: (i32, i32)) -> (i32, i32) {
        let row = |y: f64| ((y - 8.0) / 16.0).ceil();
        let first = row(self.v[0].1).max(f64::from(band.0)) as i32;
        let end = row(self.v[2].1).min(f64::from(band.1)) as i32;
        (first, end)
    }

    /// Pixels `xs..xe` of row `y`, inside `0..width`.
    fn extent(&self, y: i32, width: i32) -> (i32, i32) {
        let yc = f64::from(y) * 16.0 + 8.0;
        let long = self.v[0].0 + (yc - self.v[0].1) * self.slope[0];
        let short = if yc < self.v[1].1 {
            self.v[0].0 + (yc - self.v[0].1) * self.slope[1]
        } else {
            self.v[1].0 + (yc - self.v[1].1) * self.slope[2]
        };
        let (l, r) = if long < short {
            (long, short)
        } else {
            (short, long)
        };
        let col = |x: f64| ((x - 8.0) / 16.0).ceil();
        (col(l).max(0.0) as i32, col(r).min(f64::from(width)) as i32)
    }
}

/// Whether the triangle covers any pixel centre of the band. A far triangle is
/// often smaller than a pixel and falls between them, and its setup can be spared.
pub(super) fn covers_a_pixel(corners: &[Corner; 3], band: (i32, i32), width: i32) -> bool {
    let rows = Rows::new(corners);
    let (first, end) = rows.range(band);
    (first..end).any(|y| {
        let (xs, xe) = rows.extent(y, width);
        xs < xe
    })
}

/// Runs `row(y, xs, xe)` for each covered row of the band.
pub(super) fn each_row(
    corners: &[Corner; 3],
    band: (i32, i32),
    width: i32,
    mut row: impl FnMut(i32, i32, i32),
) {
    let rows = Rows::new(corners);
    let (first, end) = rows.range(band);
    for y in first..end {
        let (xs, xe) = rows.extent(y, width);
        if xs < xe {
            row(y, xs, xe);
        }
    }
}

/// Twice the signed area in square pixels; negative for a triangle whose air side
/// faces the eye, as the screen's Y points down.
pub(super) fn area(c: &[Corner; 3]) -> f64 {
    let px = |v: i32| f64::from(v) / f64::from(SUBPIXEL);
    let (ax, ay) = (px(c[1].x - c[0].x), px(c[1].y - c[0].y));
    let (bx, by) = (px(c[2].x - c[0].x), px(c[2].y - c[0].y));
    ax * by - bx * ay
}

/// A triangle in front of the near plane, in up to two pieces.
pub(super) fn clip_near(cam: &Camera, c: &[Corner; 3]) -> [Option<[Corner; 3]>; 2] {
    let mut out = [Corner::default(); 4];
    let mut n = 0;
    for k in 0..3 {
        let (a, b) = (&c[k], &c[(k + 1) % 3]);
        if a.iz > 0.0 {
            out[n] = *a;
            n += 1;
        }
        if (a.iz > 0.0) != (b.iz > 0.0) {
            // Always from the inside end, so a neighbour gets the same bits.
            let (i, o) = if a.iz > 0.0 { (a, b) } else { (b, a) };
            if let Some(v) = cut(cam, i, o) {
                out[n] = v;
                n += 1;
            }
        }
    }
    match n {
        3 => [Some([out[0], out[1], out[2]]), None],
        4 => [
            Some([out[0], out[1], out[2]]),
            Some([out[0], out[2], out[3]]),
        ],
        _ => [None, None],
    }
}

/// The point where the edge from `inside` to `outside` meets the near plane.
fn cut(cam: &Camera, inside: &Corner, outside: &Corner) -> Option<Corner> {
    let (ci, co) = (
        cam.world_to_camera(inside.pos),
        cam.world_to_camera(outside.pos),
    );
    let (di, do_) = (-ci.z, -co.z);
    let t = (di - cam.near()) / (di - do_);
    let pos = inside.pos.lerp(outside.pos, t);
    let s = cam.project(ci.lerp(co, t))?;
    Some(Corner {
        x: snap(s.x),
        y: snap(s.y),
        iz: s.inv_z,
        pos,
        light: inside.light.lerp(outside.light, t),
        dir: inside.dir.lerp(outside.dir, t),
        world_dir: inside.world_dir.lerp(outside.world_dir, t),
        normal: inside.normal.lerp(outside.normal, t),
        weight: inside.weight.lerp(outside.weight, t),
    })
}
