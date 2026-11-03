//! The 3D view's overlay: the face tool's faces tinted and outlined, and the
//! selected brush's faces outlined, on top of the rendered frame. It is drawn
//! with the frame's depth buffer, so a face behind a wall stays hidden, and it
//! needs no new rendering path.

use mscent_math::Vec3;
use mscent_render::{Camera, Framebuffer};

use crate::model::facetex::Marks;

/// What to mark, shared with the render thread.
pub type Highlights = std::sync::Arc<Marks>;

/// Tint colour and its opacity out of 256.
const TINT: (u32, u32) = (0x00ff_a020, 80);
/// Outline colour.
const EDGE: u32 = 0x00ff_e070;
/// A pixel counts as visible when its 1/z is at least this fraction of the
/// stored depth (the face itself wrote that depth).
const FILL_TOLERANCE: f32 = 0.985;
/// Outlines sit on the border between faces, so they get more slack.
const EDGE_TOLERANCE: f32 = 0.94;
/// Eye-plane distance polygons are clipped to.
const NEAR: f32 = 0.05;

/// A projected corner: screen position and 1/z.
#[derive(Clone, Copy)]
struct P {
    x: f32,
    y: f32,
    inv_z: f32,
    on_near: bool,
}

/// Draws `marks` over `fb` as seen by `cam`.
pub fn draw(fb: &mut Framebuffer, cam: &Camera, marks: &Marks) {
    let polys = marks.tinted.iter().map(|p| (p, true));
    for (poly, tinted) in polys.chain(marks.outlined.iter().map(|p| (p, false))) {
        let pts = project(cam, poly);
        if pts.len() < 3 {
            continue;
        }
        for i in (1..pts.len() - 1).filter(|_| tinted) {
            fill(fb, [pts[0], pts[i], pts[i + 1]]);
        }
        for i in 0..pts.len() {
            let (a, b) = (pts[i], pts[(i + 1) % pts.len()]);
            if !(a.on_near && b.on_near) {
                line(fb, a, b);
            }
        }
    }
}

/// Clips to the eye plane and projects.
fn project(cam: &Camera, poly: &[Vec3]) -> Vec<P> {
    let local: Vec<Vec3> = poly.iter().map(|&p| cam.world_to_camera(p)).collect();
    let inside = |p: Vec3| -p.z >= NEAR;
    let mut clipped: Vec<(Vec3, bool)> = Vec::with_capacity(local.len() + 2);
    for i in 0..local.len() {
        let (a, b) = (local[i], local[(i + 1) % local.len()]);
        if inside(a) {
            clipped.push((a, false));
        }
        if inside(a) != inside(b) {
            let t = (-NEAR - a.z) / (b.z - a.z);
            clipped.push((a + (b - a) * t, true));
        }
    }
    clipped
        .into_iter()
        .filter_map(|(p, on_near)| {
            let s = cam.project(p)?;
            Some(P {
                x: s.x,
                y: s.y,
                inv_z: s.inv_z,
                on_near,
            })
        })
        .collect()
}

fn visible(fb: &Framebuffer, x: i32, y: i32, inv_z: f32, tolerance: f32) -> bool {
    fb.depth_at(x, y)
        .is_some_and(|d| d <= 0.0 || inv_z >= d * tolerance)
}

fn blend(dst: u32, src: u32, alpha: u32) -> u32 {
    let mix = |shift: u32| {
        let (d, s) = ((dst >> shift) & 255, (src >> shift) & 255);
        (d * (256 - alpha) + s * alpha) >> 8
    };
    0xff00_0000 | (mix(16) << 16) | (mix(8) << 8) | mix(0)
}

fn put(fb: &mut Framebuffer, x: i32, y: i32, colour: u32, alpha: u32) {
    let w = fb.width() as i32;
    if x < 0 || y < 0 || x >= w || y >= fb.height() as i32 {
        return;
    }
    let i = (y * w + x) as usize;
    if let Some(px) = fb.pixels_mut().get_mut(i) {
        *px = blend(*px, colour, alpha);
    }
}

/// Tints a screen triangle where the face is not hidden.
fn fill(fb: &mut Framebuffer, t: [P; 3]) {
    let edge = |a: P, b: P, x: f32, y: f32| (b.x - a.x) * (y - a.y) - (b.y - a.y) * (x - a.x);
    let area = edge(t[0], t[1], t[2].x, t[2].y);
    if area.abs() < 1.0e-6 {
        return;
    }
    let lo = |f: fn(&P) -> f32| t.iter().map(f).fold(f32::INFINITY, f32::min);
    let hi = |f: fn(&P) -> f32| t.iter().map(f).fold(f32::NEG_INFINITY, f32::max);
    let (x0, x1) = (lo(|p| p.x).floor().max(0.0), hi(|p| p.x).ceil());
    let (y0, y1) = (lo(|p| p.y).floor().max(0.0), hi(|p| p.y).ceil());
    let (x1, y1) = (x1.min(fb.width() as f32), y1.min(fb.height() as f32));
    let mut y = y0 as i32;
    while (y as f32) < y1 {
        let mut x = x0 as i32;
        while (x as f32) < x1 {
            let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
            let w0 = edge(t[1], t[2], px, py) / area;
            let w1 = edge(t[2], t[0], px, py) / area;
            let w2 = 1.0 - w0 - w1;
            if w0 >= 0.0 && w1 >= 0.0 && w2 >= 0.0 {
                let inv_z = w0 * t[0].inv_z + w1 * t[1].inv_z + w2 * t[2].inv_z;
                if visible(fb, x, y, inv_z, FILL_TOLERANCE) {
                    put(fb, x, y, TINT.0, TINT.1);
                }
            }
            x += 1;
        }
        y += 1;
    }
}

/// A two pixel wide line, hidden where the face is.
fn line(fb: &mut Framebuffer, a: P, b: P) {
    let steps = (b.x - a.x).abs().max((b.y - a.y).abs()).ceil().min(4096.0) as i32;
    for i in 0..=steps {
        let t = if steps == 0 {
            0.0
        } else {
            i as f32 / steps as f32
        };
        let x = (a.x + (b.x - a.x) * t).round() as i32;
        let y = (a.y + (b.y - a.y) * t).round() as i32;
        let inv_z = a.inv_z + (b.inv_z - a.inv_z) * t;
        for (dx, dy) in [(0, 0), (1, 0), (0, 1)] {
            if visible(fb, x + dx, y + dy, inv_z, EDGE_TOLERANCE) {
                put(fb, x + dx, y + dy, EDGE, 256);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cam() -> Camera {
        let view = mscent_render::View::from_angles(Vec3::ZERO, 0.0, 0.0, 0.0);
        view.camera(1.3, 0.1, 100.0, 64, 64).expect("camera")
    }

    fn tinted(polys: Vec<Vec<Vec3>>) -> Marks {
        Marks {
            tinted: polys,
            outlined: Vec::new(),
        }
    }

    fn quad(y: f32) -> Vec<Vec3> {
        vec![
            Vec3::new(-1.0, y, -1.0),
            Vec3::new(1.0, y, -1.0),
            Vec3::new(1.0, y, 1.0),
            Vec3::new(-1.0, y, 1.0),
        ]
    }

    #[test]
    fn a_face_ahead_is_tinted_inside_and_outlined_at_its_border() {
        let mut fb = Framebuffer::new(64, 64).expect("fb");
        fb.clear(0xff20_2020);
        draw(&mut fb, &cam(), &tinted(vec![quad(3.0)]));
        let centre = fb.pixel(32, 32).expect("pixel");
        assert!(
            centre != 0xff20_2020 && (centre & 0xff) < 0x40,
            "{centre:x}"
        );
        // The outline is the full edge colour somewhere on the border row.
        let edge = (0..64)
            .filter_map(|x| fb.pixel(x, 32))
            .any(|p| p == (EDGE | 0xff00_0000));
        assert!(edge, "no outline pixel on the middle row");
        assert_eq!(fb.pixel(1, 1), Some(0xff20_2020), "far corner untouched");
    }

    #[test]
    fn faces_behind_the_eye_and_half_clipped_ones_do_not_panic() {
        let mut fb = Framebuffer::new(64, 64).expect("fb");
        fb.clear(0xff20_2020);
        draw(&mut fb, &cam(), &tinted(vec![quad(-3.0)]));
        assert!(fb.pixels().iter().all(|&p| p == 0xff20_2020));
        // A floor quad crossing the eye plane still draws its visible part.
        let floor = vec![
            Vec3::new(-1.0, -2.0, -0.5),
            Vec3::new(1.0, -2.0, -0.5),
            Vec3::new(1.0, 6.0, -0.5),
            Vec3::new(-1.0, 6.0, -0.5),
        ];
        draw(&mut fb, &cam(), &tinted(vec![floor]));
        assert!(fb.pixels().iter().any(|&p| p != 0xff20_2020));
    }

    #[test]
    fn an_outlined_face_keeps_its_inside_and_draws_its_border() {
        let mut fb = Framebuffer::new(64, 64).expect("fb");
        fb.clear(0xff20_2020);
        let marks = Marks {
            tinted: Vec::new(),
            outlined: vec![quad(3.0)],
        };
        draw(&mut fb, &cam(), &marks);
        assert_eq!(
            fb.pixel(32, 32),
            Some(0xff20_2020),
            "the inside is not tinted"
        );
        let edge = (0..64)
            .filter_map(|x| fb.pixel(x, 32))
            .any(|p| p == (EDGE | 0xff00_0000));
        assert!(edge, "no outline pixel on the middle row");
    }
}
