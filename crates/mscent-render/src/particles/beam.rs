//! Streaks: thin additive line segments whose depth varies along them.
//!
//! A streak is walked along its major screen axis, one cell per step, with
//! `1/z` and the cross-axis position both linear in the step (they are affine
//! in screen space), and a cross-section of 1 to 5 pixels from a small profile
//! table. Each pixel is depth tested on its own.
//!
//! Endpoints come from untrusted data and may be astronomically far away, so
//! the walk is limited to the band before anything is plotted, and a segment that
//! misses the band is rejected, in `f64` where the differences of `f32` values
//! cannot overflow. Every pixel is computed
//! from the original endpoints, not from the clipped ones, so a band and a
//! whole frame agree on its value.

use mscent_math::Vec3;

use super::StreakDraw;
use super::kernels::add_px;
use crate::camera::{Camera, ScreenPoint};
use crate::framebuffer::Framebuffer;

/// Coverage across the line for each width, centre pixel brightest.
const PROFILE: [[u8; 5]; 5] = [
    [255, 0, 0, 0, 0],
    [255, 255, 0, 0, 0],
    [110, 255, 110, 0, 0],
    [90, 255, 255, 90, 0],
    [60, 170, 255, 170, 60],
];

/// Near-clip a world segment, project it and build the draw. `None` when it is
/// wholly behind the near plane or any coordinate is not finite.
pub fn streak_from_world(
    cam: &Camera,
    a: Vec3,
    b: Vec3,
    width: u8,
    tint: u32,
) -> Option<StreakDraw> {
    let (ca, cb) = (cam.world_to_camera(a), cam.world_to_camera(b));
    if !ca.is_finite() || !cb.is_finite() {
        return None;
    }
    let near = cam.near();
    let (da, db) = (-ca.z, -cb.z);
    let (ca, cb) = match (da >= near, db >= near) {
        (false, false) => return None,
        (true, true) => (ca, cb),
        // One end is behind: move it along the segment to the near plane,
        // always interpolating from the end in front so the result does not
        // depend on the order of the ends.
        (a_in, _) => {
            let (front, back, d_front, d_back) = if a_in {
                (ca, cb, da, db)
            } else {
                (cb, ca, db, da)
            };
            let mut hit = front.lerp(back, (d_front - near) / (d_front - d_back));
            // Exactly on the plane, so its `1/z` is exactly `1/near`.
            hit.z = -near;
            if a_in { (ca, hit) } else { (hit, cb) }
        }
    };
    let (a, b) = (cam.project(ca)?, cam.project(cb)?);
    let finite = |p: &ScreenPoint| p.x.is_finite() && p.y.is_finite() && p.inv_z.is_finite();
    (finite(&a) && finite(&b)).then(|| StreakDraw {
        a,
        b,
        width: width.clamp(1, 5),
        tint,
    })
}

/// A point of the streak in `f64`: the screen position and `1/z`.
type P3 = (f64, f64, f64);

fn lift(p: &ScreenPoint) -> Option<P3> {
    let v = (f64::from(p.x), f64::from(p.y), f64::from(p.inv_z));
    (p.x.is_finite() && p.y.is_finite() && p.inv_z.is_finite()).then_some(v)
}

pub(super) fn draw_streak(fb: &mut Framebuffer, st: &StreakDraw) {
    let (Some(a), Some(b)) = (lift(&st.a), lift(&st.b)) else {
        return;
    };
    let width = usize::from(st.width.clamp(1, 5));
    let (rows0, rows1) = fb.row_range();
    let screen = (
        (0.0, f64::from(fb.width())),
        (f64::from(rows0), f64::from(rows1)),
    );
    let x_major = (b.0 - a.0).abs() >= (b.1 - a.1).abs();
    // (major, cross) coordinates and the screen extent of each.
    let (m0, m1, c0, c1, major_box, cross_box) = if x_major {
        (a.0, b.0, a.1, b.1, screen.0, screen.1)
    } else {
        (a.1, b.1, a.0, b.0, screen.1, screen.0)
    };
    // The cross-section hangs up to `width` pixels either side of the line.
    let margin = width as f64;
    let mut t = (0.0, 1.0);
    let inside = clip_axis(m0, m1 - m0, major_box, &mut t)
        && clip_axis(
            c0,
            c1 - c0,
            (cross_box.0 - margin, cross_box.1 + margin),
            &mut t,
        );
    if !inside {
        return;
    }
    // Cells of the segment's span on the screen. Taking the span from the end
    // coordinates, not from the clipped parameters, keeps a screen-crossing
    // line of 1e30 length whole: its parameters are too close together to
    // tell the cells apart in `f64`. The walk is at most one screen side long.
    let first = m0.min(m1).floor();
    let last = m0.max(m1).ceil().max(first + 1.0).min(major_box.1);
    let first = first.max(major_box.0);
    // Both are within the screen extent now, so the casts cannot overflow.
    let (first, last) = (first as i64, last as i64);
    let steps = StepAlong {
        m0,
        inv_dm: if m1 == m0 { 0.0 } else { 1.0 / (m1 - m0) },
        c0,
        dc: c1 - c0,
        z0: a.2,
        dz: b.2 - a.2,
    };
    for cell in first..last {
        let (cross, inv_z) = steps.at(cell);
        // A float to integer cast saturates, so a far-off cross position is
        // still a valid (off-screen) row.
        let start = (cross - width as f64 * 0.5 - 0.5).ceil() as i64;
        for (k, &cover) in PROFILE[width - 1][..width].iter().enumerate() {
            let c = start.saturating_add(k as i64);
            let (x, y) = if x_major { (cell, c) } else { (c, cell) };
            plot(fb, x, y, inv_z, cover, st.tint);
        }
    }
}

/// The line as a function of the cell index along the major axis.
struct StepAlong {
    m0: f64,
    /// Reciprocal of the span; 0 for a point, which then always sits at `t = 0`.
    inv_dm: f64,
    c0: f64,
    dc: f64,
    z0: f64,
    dz: f64,
}

impl StepAlong {
    /// Cross-axis position and `1/z` at the centre of `cell`. Positions past
    /// either end take the end's values.
    fn at(&self, cell: i64) -> (f64, f32) {
        let raw = (cell as f64 + 0.5 - self.m0) * self.inv_dm;
        // A span so small its reciprocal overflows can make 0 * inf = NaN.
        let t = if raw.is_nan() {
            0.0
        } else {
            raw.clamp(0.0, 1.0)
        };
        (self.c0 + t * self.dc, (self.z0 + t * self.dz) as f32)
    }
}

/// Narrow `t` to the part of the segment `p0 + t * d` inside `lo..=hi`.
/// `false` when nothing is left.
fn clip_axis(p0: f64, d: f64, (lo, hi): (f64, f64), t: &mut (f64, f64)) -> bool {
    if d == 0.0 {
        return (lo..=hi).contains(&p0);
    }
    let (ta, tb) = ((lo - p0) / d, (hi - p0) / d);
    t.0 = t.0.max(ta.min(tb));
    t.1 = t.1.min(ta.max(tb));
    t.0 <= t.1
}

fn plot(fb: &mut Framebuffer, x: i64, y: i64, inv_z: f32, cover: u8, tint: u32) {
    let (Ok(x), Ok(y)) = (usize::try_from(x), i32::try_from(y)) else {
        return;
    };
    let Some((color, depth)) = fb.row_mut(y) else {
        return;
    };
    // `inv_z > 0` keeps a point at or behind the eye from passing against an
    // empty (0.0) depth.
    if let (Some(c), Some(&z)) = (color.get_mut(x), depth.get(x))
        && inv_z > 0.0
        && inv_z >= z
    {
        *c = add_px::<false>(*c, u32::from(cover), tint);
    }
}

#[cfg(test)]
#[path = "beam_tests.rs"]
mod beam_tests;
