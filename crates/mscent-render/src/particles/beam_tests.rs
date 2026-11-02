use super::*;
use mscent_math::epsilon::PLANE_EPSILON_F32;

const W: u32 = 100;
const H: u32 = 80;

fn camera() -> Camera {
    // At the origin looking along +Y, Z up.
    Camera::look_at(
        Vec3::new(0.0, 0.0, 0.0),
        Vec3::new(0.0, 10.0, 0.0),
        Vec3::new(0.0, 0.0, 1.0),
        1.2,
        0.1,
        100.0,
        W,
        H,
    )
    .unwrap()
}

fn pt(x: f32, y: f32, inv_z: f32) -> ScreenPoint {
    ScreenPoint { x, y, inv_z }
}

fn streak(a: ScreenPoint, b: ScreenPoint, width: u8) -> StreakDraw {
    StreakDraw {
        a,
        b,
        width,
        tint: 0xffff_ffff,
    }
}

fn lit(fb: &Framebuffer) -> Vec<(i32, i32)> {
    let mut out = Vec::new();
    for y in 0..H as i32 {
        for x in 0..W as i32 {
            if fb.pixel(x, y) != Some(0xff00_0000) {
                out.push((x, y));
            }
        }
    }
    out
}

#[test]
fn world_segment_in_front_projects_both_ends() {
    let cam = camera();
    let (a, b) = (Vec3::new(-1.0, 10.0, 0.5), Vec3::new(2.0, 20.0, -1.0));
    let s = streak_from_world(&cam, a, b, 3, 0xff11_2233).unwrap();
    assert_eq!(Some(s.a), cam.project(cam.world_to_camera(a)));
    assert_eq!(Some(s.b), cam.project(cam.world_to_camera(b)));
    assert_eq!((s.width, s.tint), (3, 0xff11_2233));
}

#[test]
fn width_is_clamped_to_1_to_5() {
    let cam = camera();
    let (a, b) = (Vec3::new(0.0, 5.0, 0.0), Vec3::new(1.0, 6.0, 0.0));
    assert_eq!(streak_from_world(&cam, a, b, 0, 0).unwrap().width, 1);
    assert_eq!(streak_from_world(&cam, a, b, 200, 0).unwrap().width, 5);
}

#[test]
fn segment_through_the_camera_is_clipped_at_the_near_plane() {
    let cam = camera();
    let a = Vec3::new(1.0, 10.0, 0.0);
    // Ends 10 units behind the eye.
    let b = Vec3::new(1.0, -10.0, 0.0);
    let s = streak_from_world(&cam, a, b, 1, 0).unwrap();
    assert_eq!(Some(s.a), cam.project(cam.world_to_camera(a)));
    // The new end sits on the near plane: 1/z is 1/near.
    let want = 1.0 / cam.near();
    assert!(
        (s.b.inv_z - want).abs() <= want * PLANE_EPSILON_F32,
        "{}",
        s.b.inv_z
    );
    // And on the original line: x in camera space does not change along it,
    // so the projected x offset is x * focal / near.
    let off = 1.0 * cam.focal() / cam.near();
    assert!((s.b.x - (W as f32 / 2.0 + off)).abs() <= off * PLANE_EPSILON_F32);
}

#[test]
fn clipping_works_whichever_end_is_behind() {
    let cam = camera();
    let (front, back) = (Vec3::new(0.5, 8.0, 0.2), Vec3::new(-3.0, -4.0, 1.0));
    let one = streak_from_world(&cam, front, back, 2, 7).unwrap();
    let other = streak_from_world(&cam, back, front, 2, 7).unwrap();
    assert_eq!((one.a, one.b), (other.b, other.a));
}

#[test]
fn wholly_behind_or_non_finite_gives_none() {
    let cam = camera();
    let behind = |y: f32| Vec3::new(0.0, y, 0.0);
    assert!(streak_from_world(&cam, behind(-1.0), behind(-9.0), 1, 0).is_none());
    // In front of the eye but nearer than the near plane.
    assert!(streak_from_world(&cam, behind(0.01), behind(0.05), 1, 0).is_none());
    let nan = Vec3::new(f32::NAN, 5.0, 0.0);
    assert!(streak_from_world(&cam, nan, behind(5.0), 1, 0).is_none());
    let inf = Vec3::new(0.0, f32::INFINITY, 0.0);
    assert!(streak_from_world(&cam, inf, behind(5.0), 1, 0).is_none());
    let huge = Vec3::new(f32::MAX, 5.0, f32::MAX);
    // Whatever it returns for a huge value, it is finite.
    if let Some(s) = streak_from_world(&cam, huge, behind(5.0), 1, 0) {
        assert!(s.a.x.is_finite() && s.b.x.is_finite());
    }
}

#[test]
fn horizontal_streak_lights_its_row_between_the_ends() {
    let mut fb = Framebuffer::new(W, H).unwrap();
    draw_streak(
        &mut fb,
        &streak(pt(10.5, 20.5, 0.5), pt(30.5, 20.5, 0.5), 1),
    );
    let px = lit(&fb);
    assert!(px.iter().all(|&(_, y)| y == 20));
    assert!(px.iter().all(|&(x, _)| (10..=30).contains(&x)));
    assert!(px.len() >= 20);
}

#[test]
fn width_sets_the_thickness() {
    for width in 1..=5u8 {
        let mut fb = Framebuffer::new(W, H).unwrap();
        draw_streak(
            &mut fb,
            &streak(pt(10.5, 40.5, 0.5), pt(60.5, 40.5, 0.5), width),
        );
        let rows: std::collections::BTreeSet<i32> = lit(&fb).iter().map(|&(_, y)| y).collect();
        assert_eq!(rows.len(), usize::from(width), "width {width}");
    }
}

#[test]
fn steep_and_diagonal_streaks_have_no_gaps() {
    for (a, b) in [
        (pt(20.5, 5.5, 0.5), pt(25.5, 70.5, 0.5)),
        (pt(5.5, 5.5, 0.5), pt(90.5, 70.5, 0.5)),
        (pt(90.5, 70.5, 0.5), pt(5.5, 5.5, 0.5)),
    ] {
        let mut fb = Framebuffer::new(W, H).unwrap();
        draw_streak(&mut fb, &streak(a, b, 1));
        let px = lit(&fb);
        let (dx, dy) = ((b.x - a.x).abs(), (b.y - a.y).abs());
        // One pixel per step along the major axis, give or take the ends.
        let steps = dx.max(dy) as usize;
        assert!(
            px.len() >= steps && px.len() <= steps + 2,
            "{} vs {steps}",
            px.len()
        );
    }
}

#[test]
fn streak_is_clipped_by_depth_per_pixel() {
    // The right half of the screen has a wall nearer than the streak.
    let mut fb = Framebuffer::new(W, H).unwrap();
    {
        let (_, depth) = fb.planes_mut();
        for (i, d) in depth.iter_mut().enumerate() {
            *d = if (i % W as usize) < 50 { 0.0 } else { 0.9 };
        }
    }
    draw_streak(&mut fb, &streak(pt(5.5, 30.5, 0.5), pt(95.5, 30.5, 0.5), 3));
    let px = lit(&fb);
    assert!(!px.is_empty());
    assert!(px.iter().all(|&(x, _)| x < 50), "nothing behind the wall");
    assert!(px.iter().any(|&(x, _)| x > 40));
}

#[test]
fn depth_varies_along_the_streak() {
    // Streak recedes from 1/z 1.0 to 0.0 across a wall at 0.5: only the part
    // nearer than the wall shows.
    let mut fb = Framebuffer::new(W, H).unwrap();
    fb.planes_mut().1.fill(0.5);
    draw_streak(&mut fb, &streak(pt(0.5, 30.5, 1.0), pt(99.5, 30.5, 0.0), 1));
    let xs: Vec<i32> = lit(&fb).iter().map(|&(x, _)| x).collect();
    assert!(!xs.is_empty());
    assert!(xs.iter().all(|&x| x <= 50), "got {xs:?}");
    assert!(xs.contains(&45));
}

#[test]
fn huge_streak_is_clipped_before_walking_and_stays_on_the_line() {
    let mut fb = Framebuffer::new(W, H).unwrap();
    let t = std::time::Instant::now();
    // A diagonal through the screen centre at +-1e30.
    let s = streak(pt(-1.0e30, -0.8e30, 0.5), pt(1.0e30, 0.8e30, 0.5), 1);
    draw_streak(&mut fb, &s);
    assert!(t.elapsed().as_millis() < 500);
    // The line y = 0.8 x passes through the origin, not the screen, so the
    // part on screen is a stub through (0, 0) only if at all; nothing panics
    // and nothing lands far from it.
    assert!(lit(&fb).iter().all(|&(x, y)| x < 5 && y < 5));
}

#[test]
fn huge_streak_through_the_screen_crosses_it() {
    let mut fb = Framebuffer::new(W, H).unwrap();
    // Horizontal, enormous, and exactly at row 40.
    let s = streak(pt(-1.0e30, 40.5, 0.5), pt(1.0e30, 40.5, 0.5), 1);
    draw_streak(&mut fb, &s);
    let px = lit(&fb);
    assert!(px.iter().all(|&(_, y)| y == 40));
    assert_eq!(px.len(), W as usize);
}

#[test]
fn zero_length_streak_is_a_dot() {
    let mut fb = Framebuffer::new(W, H).unwrap();
    draw_streak(
        &mut fb,
        &streak(pt(50.5, 40.5, 0.5), pt(50.5, 40.5, 0.5), 1),
    );
    assert_eq!(lit(&fb), vec![(50, 40)]);
}

#[test]
fn non_finite_streak_draws_nothing() {
    let mut fb = Framebuffer::new(W, H).unwrap();
    for bad in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        draw_streak(&mut fb, &streak(pt(bad, 10.0, 0.5), pt(50.0, 10.0, 0.5), 1));
        draw_streak(&mut fb, &streak(pt(0.0, 10.0, bad), pt(50.0, 10.0, 0.5), 1));
    }
    assert!(lit(&fb).is_empty());
}

#[test]
fn a_streak_ending_on_a_band_edge_draws_nothing_in_the_next_band() {
    let s = streak(pt(50.5, 23.2, 0.5), pt(50.5, 24.0, 0.5), 1);
    let mut below = Framebuffer::new_band(W, 24, 24, H).unwrap();
    let before = below.pixels().to_vec();
    draw_streak(&mut below, &s);
    assert_eq!(
        below.pixels(),
        &before[..],
        "the whole frame walks row 23 only"
    );
    let mut above = Framebuffer::new_band(W, 0, 24, H).unwrap();
    let before = above.pixels().to_vec();
    draw_streak(&mut above, &s);
    assert_ne!(above.pixels(), &before[..]);
}
