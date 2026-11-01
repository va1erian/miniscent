//! Shadow crawl and sawtooth metric shared by `examples/crawl.rs` and
//! `tests/crawl.rs`.
//!
//! `stability.rs` looks at one screen pixel over time and skips every pixel
//! that a real edge crosses, which is exactly where a shadow edge crawls. This
//! metric follows fixed points of the WORLD instead. A rig names a straight
//! shadow edge on a floor (the line a pillar corner throws away from a lamp),
//! a camera that looks at it from a given distance, and the points to follow:
//! a grid across the edge. The camera then moves by a fraction of a pixel per
//! frame (dolly, strafe or yaw) through the same `LevelRenderer::render` call
//! the game makes, at the internal size the adaptive controller would pick.
//!
//! Three numbers come out, all in screen units:
//!
//! * `lum_rms`: temporal noise of the luminance of the followed points. The
//!   true shading of a world point does not change when the camera moves, so
//!   the residual of its luminance after removing a low-order polynomial over
//!   the sequence is crawl (plus the fixed dither, about 0.3 levels).
//! * `contour_px`: the same for the position of the 50% contour of the shadow
//!   edge across the edge, in pixels.
//! * `saw_px`: within one frame, the RMS deviation of that contour from the
//!   best straight line, in pixels: the stair-step of the edge.

use core::f32::consts::FRAC_1_SQRT_2;

use mscent_math::Vec3;
use mscent_render::{Camera, Framebuffer, LevelRenderer, Quality, View};

pub const FRAMES: usize = 64;
/// Fine step (1/16 pixel per frame) and coarse step (1/4 pixel per frame).
pub const FINE: f32 = 1.0 / 16.0;
pub const COARSE: f32 = 1.0 / 4.0;
/// Internal sizes in the game: full, 750 per-mille and 500 per-mille of 640x360.
pub const FULL: (u32, u32) = (640, 360);
pub const REDUCED: (u32, u32) = (480, 270);
pub const LOW: (u32, u32) = (320, 180);
/// Polynomial degree removed before the residual is taken.
const DEGREE: usize = 3;
/// Samples along the edge and across it.
const ALONG: usize = 41;
const ACROSS: usize = 49;
/// Half width of the profile across the edge, in world units.
const REACH: f32 = 1.2;
const MERGE: f32 = 4.0; // samples: crossings closer than this are one edge
/// A profile whose two ends differ by less than this is not an edge.
const MIN_STEP: f32 = 8.0;

/// A straight floor shadow edge and the camera that watches it.
#[derive(Clone, Copy, Debug)]
pub struct Rig {
    pub name: &'static str,
    /// Floor point where the edge starts, and its unit direction.
    pub origin: [f32; 2],
    pub dir: [f32; 2],
    /// The camera looks at `origin + dir * centre`, from `dist` away.
    pub centre: f32,
    /// Half length of the part of the edge that is measured.
    pub half: f32,
    /// Unit horizontal vector from the look-at point to the eye.
    pub eye_from: [f32; 2],
    pub dist: f32,
    pub eye_z: f32,
}

/// The pillar edges of the start room (lamp at (6, 6), pillar corner
/// (13, 11)) at three distances, and the cave (lamp at (43, 44), pillar
/// corner (40, 41)) at one.
pub fn rigs() -> Vec<Rig> {
    let a = |name, centre, half, dist| Rig {
        name,
        origin: [13.0, 11.0],
        dir: [0.8137, 0.5812],
        centre,
        half,
        eye_from: [0.0, -1.0],
        dist,
        eye_z: 2.5,
    };
    vec![
        Rig {
            eye_z: 1.2,
            ..a("room_close", 4.0, 0.7, 2.0)
        },
        a("room_near", 4.0, 1.0, 3.0),
        a("room_mid", 4.0, 2.0, 7.0),
        a("room_far", 4.0, 2.5, 12.0),
        Rig {
            name: "cave_mid",
            origin: [40.0, 41.0],
            dir: [-FRAC_1_SQRT_2, -FRAC_1_SQRT_2],
            centre: 5.0,
            half: 2.5,
            eye_from: [0.94, -0.34],
            dist: 8.0,
            eye_z: 2.2,
        },
    ]
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Motion {
    Dolly,
    Strafe,
    Yaw,
}

pub const MOTIONS: [Motion; 3] = [Motion::Dolly, Motion::Strafe, Motion::Yaw];

fn v3(p: [f32; 2], z: f32) -> Vec3 {
    Vec3::new(p[0], p[1], z)
}

impl Rig {
    fn point(&self, t: f32, s: f32) -> Vec3 {
        let n = [-self.dir[1], self.dir[0]];
        v3(
            [
                self.origin[0] + self.dir[0] * t + n[0] * s,
                self.origin[1] + self.dir[1] * t + n[1] * s,
            ],
            0.0,
        )
    }

    /// The view `step` frames into a motion of `px` pixels per frame.
    pub fn view(&self, motion: Motion, step: usize, px: f32, fov: f32, h: u32) -> View {
        let target = self.point(self.centre, 0.0);
        let eye0 = Vec3::new(
            target.x + self.eye_from[0] * self.dist,
            target.y + self.eye_from[1] * self.dist,
            self.eye_z,
        );
        let fwd = [-self.eye_from[0], -self.eye_from[1]];
        let yaw = (-fwd[0]).atan2(fwd[1]);
        let pitch = -self.eye_z.atan2(self.dist);
        let rad_per_px = 2.0 * (0.5 * fov).tan() / h as f32;
        let t = step as f32 * px;
        match motion {
            Motion::Dolly => {
                let d = self.dist * rad_per_px * t;
                let eye = eye0 + Vec3::new(fwd[0], fwd[1], 0.0) * d;
                View::from_angles(eye, yaw, pitch, 0.0)
            }
            Motion::Strafe => {
                let d = self.dist * rad_per_px * t;
                let eye = eye0 + Vec3::new(yaw.cos(), yaw.sin(), 0.0) * d;
                View::from_angles(eye, yaw, pitch, 0.0)
            }
            Motion::Yaw => View::from_angles(eye0, yaw - rad_per_px * t, pitch, 0.0),
        }
    }
}

/// Luminance image with bilinear lookup at continuous pixel coordinates.
pub struct Luma {
    w: usize,
    h: usize,
    v: Vec<f32>,
}

impl Luma {
    pub fn of(fb: &Framebuffer) -> Self {
        let luma = |c: u32| {
            let (r, g, b) = ((c >> 16) & 0xff, (c >> 8) & 0xff, c & 0xff);
            0.299 * r as f32 + 0.587 * g as f32 + 0.114 * b as f32
        };
        Self {
            w: fb.width() as usize,
            h: fb.height() as usize,
            v: fb.pixels().iter().map(|&c| luma(c)).collect(),
        }
    }

    /// Bilinear value at `(x, y)`, pixel centres at half integers.
    pub fn at(&self, x: f32, y: f32) -> f32 {
        let (fx, fy) = (x - 0.5, y - 0.5);
        let (x0, y0) = (fx.floor(), fy.floor());
        let (tx, ty) = (fx - x0, fy - y0);
        let px = |dx: i32, dy: i32| {
            let ix = (x0 as i32 + dx).clamp(0, self.w as i32 - 1) as usize;
            let iy = (y0 as i32 + dy).clamp(0, self.h as i32 - 1) as usize;
            self.v[iy * self.w + ix]
        };
        let top = px(0, 0) * (1.0 - tx) + px(1, 0) * tx;
        let bottom = px(0, 1) * (1.0 - tx) + px(1, 1) * tx;
        top * (1.0 - ty) + bottom * ty
    }
}

/// Orthonormal polynomial basis over `n` equally spaced samples.
fn basis(n: usize, degree: usize) -> Vec<Vec<f32>> {
    let mut out: Vec<Vec<f32>> = Vec::new();
    for d in 0..=degree {
        let mut q: Vec<f32> = (0..n)
            .map(|i| (2.0 * i as f32 / (n - 1) as f32 - 1.0).powi(d as i32))
            .collect();
        for b in &out {
            let dot: f32 = q.iter().zip(b).map(|(x, y)| x * y).sum();
            q.iter_mut().zip(b).for_each(|(x, y)| *x -= dot * y);
        }
        let norm = q.iter().map(|x| x * x).sum::<f32>().sqrt();
        q.iter_mut().for_each(|x| *x /= norm);
        out.push(q);
    }
    out
}

/// RMS of `series` after removing its least-squares polynomial (the basis
/// from [`basis`] with the same length).
fn detrend_rms(series: &[f32], basis: &[Vec<f32>]) -> f32 {
    let mut r = series.to_vec();
    for b in basis {
        let dot: f32 = r.iter().zip(b).map(|(x, y)| x * y).sum();
        r.iter_mut().zip(b).for_each(|(x, y)| *x -= dot * y);
    }
    (r.iter().map(|x| x * x).sum::<f32>() / r.len() as f32).sqrt()
}

/// RMS distance of `(x, y)` pairs from their best straight line (least
/// squares on the perpendicular offset, with the points parameterised by
/// index, which is monotone along the edge).
pub fn line_rms(points: &[(f32, f32)]) -> (f32, f32) {
    let n = points.len() as f32;
    if points.len() < 3 {
        return (0.0, 0.0);
    }
    let (mx, my) = points
        .iter()
        .fold((0.0, 0.0), |a, p| (a.0 + p.0 / n, a.1 + p.1 / n));
    let (mut sxx, mut sxy, mut syy) = (0.0, 0.0, 0.0);
    for p in points {
        let (dx, dy) = (p.0 - mx, p.1 - my);
        (sxx, sxy, syy) = (sxx + dx * dx, sxy + dx * dy, syy + dy * dy);
    }
    // Smallest eigenvalue of the scatter matrix is the squared perpendicular
    // spread.
    let tr = sxx + syy;
    let det = sxx * syy - sxy * sxy;
    let small = tr / 2.0 - (tr * tr / 4.0 - det).max(0.0).sqrt();
    // Principal axis angle: tan(2a) = 2 sxy / (sxx - syy); signed offsets.
    let a = 0.5 * (2.0 * sxy).atan2(sxx - syy);
    let (nx, ny) = (-a.sin(), a.cos());
    let offs = points.iter().map(|p| (p.0 - mx) * nx + (p.1 - my) * ny);
    let (lo, hi) = offs.fold((f32::MAX, f32::MIN), |m, o| (m.0.min(o), m.1.max(o)));
    ((small.max(0.0) / n).sqrt(), hi - lo)
}

/// What one rig, motion and size measured.
#[derive(Clone, Copy, Debug, Default)]
pub struct Crawl {
    /// Luminance noise (levels), mean over all followed points and over the
    /// ones within 0.25 units of the edge.
    pub lum_rms: f32,
    pub lum_rms_edge: f32,
    /// Crawl of the edge contour across the edge (pixels), mean and worst.
    pub contour_px: f32,
    pub contour_max: f32,
    /// Stair-step of the contour within a frame (pixels), mean over frames.
    pub saw_px: f32,
    /// Largest peak-to-peak deviation of the contour from its line in a frame.
    pub saw_pp: f32,
    /// Followed points and contour samples that were usable.
    pub points: usize,
    pub contour_samples: usize,
}

struct FrameData {
    /// Luminance at each followed point.
    lum: Vec<f32>,
    /// Per contour sample: offset of the contour from the projected analytic
    /// edge (pixels, across the edge) and its screen position.
    contour: Vec<Option<(f32, (f32, f32))>>,
}

fn camera_of(r: &LevelRenderer, view: &View, size: (u32, u32)) -> Camera {
    view.camera(r.config().fov_y, 0.1, 400.0, size.0, size.1)
        .expect("camera")
}

fn screen(cam: &Camera, p: Vec3) -> Option<(f32, f32)> {
    cam.project(cam.world_to_camera(p)).map(|s| (s.x, s.y))
}

/// Grid of followed points: five along the edge, seven across it.
fn grid(rig: &Rig) -> Vec<(f32, f32)> {
    let mut g = Vec::new();
    for i in 0..5 {
        let t = rig.centre + rig.half * (i as f32 - 2.0) / 2.0;
        for j in 0..7 {
            g.push((t, (j as f32 - 3.0) * 0.25));
        }
    }
    g
}

fn analyse(rig: &Rig, cam: &Camera, img: &Luma) -> Option<FrameData> {
    let lum = grid(rig)
        .iter()
        .map(|&(t, s)| {
            let p = screen(cam, rig.point(t, s))?;
            Some(img.at(p.0, p.1))
        })
        .collect::<Option<Vec<f32>>>()?;
    let mut contour = Vec::with_capacity(ALONG);
    for k in 0..ALONG {
        let t = rig.centre + rig.half * (2.0 * k as f32 / (ALONG - 1) as f32 - 1.0);
        let profile: Option<Vec<f32>> = (0..ACROSS)
            .map(|j| {
                let s = REACH * (2.0 * j as f32 / (ACROSS - 1) as f32 - 1.0);
                let p = screen(cam, rig.point(t, s))?;
                Some(img.at(p.0, p.1))
            })
            .collect();
        let edge_pt = screen(cam, rig.point(t, 0.0))?;
        contour.push(profile.and_then(|p| crossing(&p)).and_then(|s| {
            let at = screen(cam, rig.point(t, s))?;
            // Across-edge direction on screen: the projected edge tangent
            // rotated by 90 degrees.
            let a = screen(cam, rig.point(t - 0.5, 0.0))?;
            let b = screen(cam, rig.point(t + 0.5, 0.0))?;
            let len = ((b.0 - a.0).powi(2) + (b.1 - a.1).powi(2)).sqrt().max(1e-6);
            let (ux, uy) = ((b.0 - a.0) / len, (b.1 - a.1) / len);
            let across = (at.0 - edge_pt.0) * -uy + (at.1 - edge_pt.1) * ux;
            Some((across, at))
        }));
    }
    Some(FrameData { lum, contour })
}

/// Position (world units, interpolated) where a profile crosses the middle of
/// its two plateaus, the crossing nearest the profile centre.
fn crossing(p: &[f32]) -> Option<f32> {
    let m = 4;
    let avg = |s: &[f32]| s.iter().sum::<f32>() / s.len() as f32;
    let (a, b) = (avg(&p[..m]), avg(&p[p.len() - m..]));
    if (a - b).abs() < MIN_STEP {
        return None;
    }
    let mid = 0.5 * (a + b);
    let centre = (p.len() - 1) as f32 / 2.0;
    let mut best: Option<f32> = None;
    let mut last = f32::NEG_INFINITY;
    for (j, w) in p.windows(2).enumerate() {
        if (w[0] - mid) * (w[1] - mid) <= 0.0 && w[0] != w[1] {
            let x = j as f32 + (mid - w[0]) / (w[1] - w[0]);
            // A second, separate crossing means texture detail (a grout
            // line) crosses the profile: the contour is ambiguous and its
            // jumps between frames would be read as crawl of the shadow.
            if x - last > MERGE && last.is_finite() {
                return None;
            }
            last = x;
            if best.is_none_or(|b| (x - centre).abs() < (b - centre).abs()) {
                best = Some(x);
            }
        }
    }
    best.map(|x| REACH * (2.0 * x / (p.len() - 1) as f32 - 1.0))
}

/// Everything the example wants to draw: the frames and where the points are.
pub struct Run {
    pub crawl: Crawl,
    pub frames: Vec<Framebuffer>,
    pub points: Vec<Vec<(f32, f32)>>,
}

/// Render `FRAMES` frames of `motion` at `px` pixels per frame, at the
/// internal `size`, and measure. `keep` frames are kept for PNGs (the first,
/// the last and one in between).
pub fn run(
    r: &LevelRenderer,
    rig: &Rig,
    motion: Motion,
    q: Quality,
    size: (u32, u32),
    px: f32,
    keep: bool,
) -> Option<Run> {
    let mut fb = Framebuffer::new(size.0, size.1)?;
    let fov = r.config().fov_y;
    let mut data = Vec::new();
    let mut kept = Vec::new();
    let mut pts = Vec::new();
    for step in 0..FRAMES {
        let view = rig.view(motion, step, px, fov, size.1);
        r.render(&mut fb, &view, q, &[], None)?;
        let cam = camera_of(r, &view, size);
        data.push(analyse(rig, &cam, &Luma::of(&fb))?);
        if keep && (step == 0 || step == FRAMES / 2 || step == FRAMES - 1) {
            let mut copy = Framebuffer::new(size.0, size.1)?;
            copy.pixels_mut().copy_from_slice(fb.pixels());
            kept.push(copy);
            pts.push(
                grid(rig)
                    .iter()
                    .filter_map(|&(t, s)| screen(&cam, rig.point(t, s)))
                    .collect(),
            );
        }
    }
    Some(Run {
        crawl: summarize(&data),
        frames: kept,
        points: pts,
    })
}

fn summarize(data: &[FrameData]) -> Crawl {
    let b = basis(data.len(), DEGREE);
    let n_pts = data[0].lum.len();
    let grid_edge: Vec<bool> = (0..n_pts).map(|i| (2..=4).contains(&(i % 7))).collect();
    let (mut sum, mut sum_edge, mut n_edge) = (0.0, 0.0, 0);
    for (i, &near_edge) in grid_edge.iter().enumerate() {
        let series: Vec<f32> = data.iter().map(|f| f.lum[i]).collect();
        let r = detrend_rms(&series, &b);
        sum += r;
        if near_edge {
            sum_edge += r;
            n_edge += 1;
        }
    }
    let (mut c_sum, mut c_max, mut c_n) = (0.0f32, 0.0f32, 0usize);
    for k in 0..ALONG {
        let series: Option<Vec<f32>> = data
            .iter()
            .map(|f| f.contour[k].map(|c| c.0))
            .collect::<Option<_>>();
        if let Some(s) = series {
            let r = detrend_rms(&s, &b);
            c_sum += r;
            c_max = c_max.max(r);
            c_n += 1;
        }
    }
    let saws: Vec<(f32, f32)> = data
        .iter()
        .filter_map(|f| {
            let pts: Vec<(f32, f32)> = f.contour.iter().flatten().map(|c| c.1).collect();
            (pts.len() > ALONG / 2).then(|| line_rms(&pts))
        })
        .collect();
    Crawl {
        lum_rms: sum / n_pts as f32,
        lum_rms_edge: sum_edge / n_edge.max(1) as f32,
        contour_px: c_sum / c_n.max(1) as f32,
        contour_max: c_max,
        saw_px: saws.iter().map(|s| s.0).sum::<f32>() / saws.len().max(1) as f32,
        saw_pp: saws.iter().map(|s| s.1).fold(0.0, f32::max),
        points: n_pts,
        contour_samples: c_n,
    }
}

/// Mean of the metric over all motions for one rig.
pub fn measure(
    r: &LevelRenderer,
    rig: &Rig,
    q: Quality,
    size: (u32, u32),
    px: f32,
) -> Option<Crawl> {
    let mut acc = Crawl::default();
    for m in MOTIONS {
        let c = run(r, rig, m, q, size, px, false)?.crawl;
        acc.lum_rms += c.lum_rms / 3.0;
        acc.lum_rms_edge += c.lum_rms_edge / 3.0;
        acc.contour_px += c.contour_px / 3.0;
        acc.contour_max = acc.contour_max.max(c.contour_max);
        acc.saw_px += c.saw_px / 3.0;
        acc.saw_pp = acc.saw_pp.max(c.saw_pp);
        acc.points = c.points;
        acc.contour_samples = acc.contour_samples.max(c.contour_samples);
    }
    Some(acc)
}

#[cfg(test)]
mod crossing_tests {
    use super::*;

    fn profile(f: impl Fn(usize) -> f32) -> Vec<f32> {
        (0..ACROSS).map(f).collect()
    }

    #[test]
    fn one_clean_edge_has_a_crossing_and_a_grout_line_does_not() {
        let edge = profile(|j| if j < 24 { 200.0 } else { 40.0 });
        let at = crossing(&edge).expect("single edge");
        assert!(at.abs() < 0.1, "edge at the centre, got {at}");
        // The same edge plus a dark line 10 samples away: two crossings.
        let grout = profile(|j| match j {
            24.. if j < 28 => 40.0,
            14 | 15 => 0.0,
            _ if j < 24 => 200.0,
            _ => 40.0,
        });
        assert_eq!(crossing(&grout), None);
    }
}
