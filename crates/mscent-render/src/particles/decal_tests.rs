use super::*;

const W: u32 = 100;
const H: u32 = 80;
const WALL: f32 = 0.5;
const GREY: u32 = 0xff80_8080;

/// A target of grey wall at `1/z = WALL`.
fn wall() -> Framebuffer {
    let mut fb = Framebuffer::new(W, H).unwrap();
    fb.pixels_mut().fill(GREY);
    fb.planes_mut().1.fill(WALL);
    fb
}

fn point(x: f32, y: f32, inv_z: f32, s: f32, t: f32) -> DecalPoint {
    DecalPoint { x, y, inv_z, s, t }
}

/// A square of half-side `r` around (50, 40), face on, at `1/z = inv_z`.
fn square(r: f32, inv_z: f32) -> DecalDraw {
    let (x0, x1, y0, y1) = (50.0 - r, 50.0 + r, 40.0 - r, 40.0 + r);
    let mut points = [DecalPoint::default(); MAX_DECAL_POINTS];
    points[..4].copy_from_slice(&[
        point(x0, y0, inv_z, 0.0, 0.0),
        point(x1, y0, inv_z, 1.0, 0.0),
        point(x1, y1, inv_z, 1.0, 1.0),
        point(x0, y1, inv_z, 0.0, 1.0),
    ]);
    DecalDraw {
        points,
        len: 4,
        darkness: 256,
        mask: Mask::Soft,
    }
}

fn draw(fb: &mut Framebuffer, d: &DecalDraw) {
    draw_decal(fb, d, &SpriteBank::new());
}

fn darkened(fb: &Framebuffer) -> Vec<(i32, i32)> {
    let all = (0..H as i32).flat_map(|y| (0..W as i32).map(move |x| (x, y)));
    all.filter(|&(x, y)| fb.pixel(x, y) != Some(GREY)).collect()
}

#[test]
fn a_decal_darkens_its_centre_most_and_stays_inside_its_polygon() {
    let mut fb = wall();
    draw(&mut fb, &square(12.0, WALL));
    let hit = darkened(&fb);
    assert!(hit.len() > 200, "{} pixels", hit.len());
    assert!(
        hit.iter()
            .all(|&(x, y)| (38..62).contains(&x) && (28..52).contains(&y))
    );
    let (centre, rim) = (fb.pixel(50, 40).unwrap(), fb.pixel(39, 40).unwrap());
    assert!(centre & 0xff < rim & 0xff, "{centre:08x} against {rim:08x}");
    assert_eq!(centre >> 24, 0xff, "stays opaque");
}

#[test]
fn it_never_writes_depth() {
    let mut fb = wall();
    draw(&mut fb, &square(12.0, WALL));
    assert!(fb.planes_mut().1.iter().all(|&z| z == WALL));
}

#[test]
fn the_bias_admits_rounding_and_refuses_anything_in_front() {
    let nearer = |gap: f32| {
        let mut fb = wall();
        draw(&mut fb, &square(12.0, WALL / (1.0 + gap)));
        darkened(&fb).len()
    };
    let level = nearer(0.0);
    assert!(level > 200);
    // Within the bias: the decal's 1/z a hair under the wall's.
    // (A few rim pixels flip on the mask lookup's rounding, so not equal.)
    assert!(nearer(DECAL_DEPTH_BIAS * 0.5) * 100 >= level * 98);
    assert!(nearer(DECAL_DEPTH_BIAS * 4.0) * 100 < level * 5);
    // A wall 5% nearer than the decal hides all of it.
    assert_eq!(nearer(0.05), 0);
}

#[test]
fn pixels_the_scene_did_not_draw_are_left_alone() {
    let mut fb = wall();
    fb.planes_mut().1.fill(0.0);
    draw(&mut fb, &square(12.0, WALL));
    assert!(darkened(&fb).is_empty());
}

#[test]
fn bands_agree_with_a_whole_frame() {
    let mut whole = wall();
    let d = square(20.0, WALL);
    draw(&mut whole, &d);
    let rows = |fb: &Framebuffer, from: u32, to: u32| {
        (from..to)
            .flat_map(|y| (0..W as i32).map(move |x| (x, y as i32)))
            .map(|(x, y)| fb.pixel(x, y))
            .collect::<Vec<_>>()
    };
    for cut in [1u32, 17, 40, 63, 79] {
        let mut top = Framebuffer::new_band(W, 0, cut, H).unwrap();
        let mut bottom = Framebuffer::new_band(W, cut, H - cut, H).unwrap();
        for band in [&mut top, &mut bottom] {
            band.pixels_mut().fill(GREY);
            band.planes_mut().1.fill(WALL);
            draw(band, &d);
        }
        assert_eq!(rows(&top, 0, cut), rows(&whole, 0, cut), "top at {cut}");
        assert_eq!(
            rows(&bottom, cut, H),
            rows(&whole, cut, H),
            "bottom at {cut}"
        );
    }
}

#[test]
fn a_tilted_decal_passes_on_its_own_plane_and_not_behind_it() {
    // 1/z grows to the right; a depth buffer holding the same plane must pass
    // everywhere, and the plane 10% further away must not.
    let tilted = |scale: f32| {
        let mut d = square(20.0, 0.0);
        for p in &mut d.points[..4] {
            p.inv_z = (0.4 + 0.004 * p.x) * scale;
        }
        d
    };
    let mut scene = wall();
    for row in scene.planes_mut().1.chunks_mut(W as usize) {
        for (x, z) in row.iter_mut().enumerate() {
            *z = 0.4 + 0.004 * (x as f32 + 0.5);
        }
    }
    let mut on_plane = scene.clone();
    draw(&mut on_plane, &tilted(1.0));
    let mut behind = scene.clone();
    draw(&mut behind, &tilted(0.9));
    assert!(darkened(&on_plane).len() > 600);
    assert!(
        darkened(&behind).is_empty(),
        "10% behind the wall is hidden"
    );
}

#[test]
fn hostile_decals_draw_nothing_and_do_not_panic() {
    let bank = SpriteBank::new();
    let mut fb = wall();
    let bad = [
        f32::NAN,
        f32::INFINITY,
        f32::NEG_INFINITY,
        1.0e30,
        -1.0e30,
        -1.0,
        0.0,
    ];
    for v in bad {
        for field in 0..5 {
            let mut d = square(12.0, WALL);
            let p = &mut d.points[1];
            match field {
                0 => p.x = v,
                1 => p.y = v,
                2 => p.inv_z = v,
                3 => p.s = v,
                _ => p.t = v,
            }
            draw_decal(&mut fb, &d, &bank);
        }
    }
    for len in [0u8, 1, 2, 17, 255] {
        let mut d = square(12.0, WALL);
        d.len = len;
        draw_decal(&mut fb, &d, &bank);
    }
    let mut d = square(12.0, WALL);
    d.darkness = u16::MAX;
    draw_decal(&mut fb, &d, &bank);
    // A sliver with no area, and a decal far larger than the screen.
    let mut flat = square(12.0, WALL);
    flat.points[2] = flat.points[1];
    flat.points[3] = flat.points[0];
    draw_decal(&mut fb, &flat, &bank);
    draw_decal(&mut fb, &square(9.0e5, WALL), &bank);
}

#[test]
fn world_decals_clip_at_the_near_plane_and_refuse_bad_values() {
    let cam = Camera::look_at(
        Vec3::ZERO,
        Vec3::new(0.0, 10.0, 0.0),
        Vec3::new(0.0, 0.0, 1.0),
        1.2,
        0.1,
        100.0,
        W,
        H,
    )
    .unwrap();
    let quad = |y0: f32, y1: f32| {
        let v = |x, y, z, s, t| DecalVertex {
            pos: Vec3::new(x, y, z),
            s,
            t,
        };
        [
            v(-1.0, y0, -1.0, 0.0, 0.0),
            v(1.0, y0, -1.0, 1.0, 0.0),
            v(1.0, y1, 1.0, 1.0, 1.0),
            v(-1.0, y1, 1.0, 0.0, 1.0),
        ]
    };
    let ahead = decal_from_world(&cam, &quad(5.0, 5.0), 200, Mask::Puff(1)).expect("visible");
    assert_eq!(ahead.len, 4);
    // Half of it behind the eye: cut at the near plane.
    let across = decal_from_world(&cam, &quad(-2.0, 4.0), 200, Mask::Soft).expect("clipped");
    assert!(across.len >= 4);
    assert!(
        across
            .points()
            .iter()
            .all(|p| p.inv_z <= 1.0 / 0.1 + 1.0e-3)
    );
    assert!(decal_from_world(&cam, &quad(-5.0, -5.0), 200, Mask::Soft).is_none());
    let mut nan = quad(5.0, 5.0);
    nan[2].pos.x = f32::NAN;
    assert!(decal_from_world(&cam, &nan, 200, Mask::Soft).is_none());
    nan[2].pos.x = 1.0;
    nan[1].s = f32::INFINITY;
    assert!(decal_from_world(&cam, &nan, 200, Mask::Soft).is_none());
    assert!(decal_from_world(&cam, &quad(5.0, 5.0)[..2], 200, Mask::Soft).is_none());
    let too_many = [quad(5.0, 5.0)[0]; MAX_DECAL_POINTS];
    assert!(decal_from_world(&cam, &too_many, 200, Mask::Soft).is_none());
    // Off to the side: nothing to draw.
    let mut side = quad(5.0, 5.0);
    side.iter_mut().for_each(|v| v.pos.x += 500.0);
    assert!(decal_from_world(&cam, &side, 200, Mask::Soft).is_none());
}

#[test]
fn the_screen_area_is_the_polygon_cut_by_the_screen() {
    let d = square(10.0, WALL);
    assert!((d.screen_area(W as f32, H as f32) - 400.0).abs() < 0.01);
    // Half of it off the left edge.
    let half = d.screen_area(50.0, H as f32);
    assert!((half - 200.0).abs() < 0.01, "{half}");
    let mut bad = square(10.0, WALL);
    bad.points[0].x = f32::NAN;
    assert_eq!(bad.screen_area(W as f32, H as f32), 0.0);
}
