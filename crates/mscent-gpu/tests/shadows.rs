//! Projected shadows, read from real pixels. Nothing here compares with the software
//! renderer, which draws none: each look is held to where a box's shadow falls by
//! hand, to the frame without shadows, and to its own steadiness.
//!
//! The room is flat grey with one dynamic light 3 above the middle of a 2 by 2 by 2
//! box hung with its top at 0. The floor is 7 below the light and 4 below the top of
//! the box, so the top's shadow is a square of half width 3 * 7 / 3 ... by similar
//! triangles, 1 * 7 / 3 = 2.33.
//!
//! A machine with no GPU skips this, unless `MSCENT_REQUIRE_GPU` is set.

#[path = "support/gpu.rs"]
mod gpu_support;
#[allow(dead_code)]
#[path = "support/shade.rs"]
mod shade;

use mscent_math::Vec3;
use mscent_render::{Actor, ActorKind, MoverPose};
use shade::{
    Contents, OFF, ON, Room, SIZE, Shot, box_at, keep, lamp, level_at, level_in, pixel_of, room,
    view,
};

fn v(x: f32, y: f32, z: f32) -> Vec3 {
    Vec3::new(x, y, z)
}

/// Where the shadow of the box's top falls: half its width, from the light's height.
const SHADOW_HALF: f32 = 7.0 / 3.0;
const FLOOR: f32 = shade::FLOOR_Z;

/// The room, its light above the box, and the box.
fn scene(
    spec: Room,
) -> (
    Shot,
    [mscent_render::DynLight; 1],
    [mscent_render::model::ModelInstance; 1],
) {
    let shot = Shot::new(&gpu_support::gpu().expect("a gpu"), room(spec));
    let light = [lamp(&shot.level, v(0.0, 0.0, 3.0))];
    (shot, light, [box_at(v(0.0, 0.0, -1.0))])
}

/// The pixel of a floor point, and its level in `frame`.
fn floor_level(shot: &Shot, frame: &[u32], x: f32, y: f32) -> f32 {
    let cam = shot.camera(&view());
    level_at(frame, pixel_of(&cam, v(x, y, FLOOR)))
}

macro_rules! with_gpu {
    () => {
        if gpu_support::gpu().is_none() {
            return;
        }
    };
}

#[test]
fn the_box_shades_the_floor_where_by_hand_it_should_and_not_beside_it() {
    with_gpu!();
    let (mut shot, lights, boxes) = scene(Room::default());
    let contents = Contents {
        lights: &lights,
        boxes: &boxes,
        ..Contents::default()
    };
    let (off, on) = (
        shot.frame(&view(), OFF, &contents),
        shot.frame(&view(), ON, &contents),
    );
    keep("geometry_off.png", &off);
    keep("geometry_on.png", &on);
    let margin = 0.9;
    let inside = SHADOW_HALF - margin;
    let outside = SHADOW_HALF + margin;
    // In front of the box and to each side of it, which the camera can see.
    for (x, y) in [
        (-inside, -inside),
        (inside, -inside),
        (0.0, -inside - 0.2),
        (-inside, 0.0),
    ] {
        let (lit, shaded) = (
            floor_level(&shot, &off, x, y),
            floor_level(&shot, &on, x, y),
        );
        assert!(lit > 40.0, "({x}, {y}) is lit by the light: {lit}");
        assert!(
            shaded < 0.1 * lit,
            "({x}, {y}) is in the shadow: {shaded} of {lit}"
        );
    }
    for (x, y) in [
        (-outside, -inside),
        (outside, -inside),
        (0.0, -outside),
        (-outside, 0.0),
    ] {
        let (lit, shaded) = (
            floor_level(&shot, &off, x, y),
            floor_level(&shot, &on, x, y),
        );
        assert!(
            (shaded - lit).abs() <= 0.03 * lit + 1.0,
            "({x}, {y}) is beside the shadow: {shaded} of {lit}"
        );
    }
}

#[test]
fn the_shadow_is_gone_when_the_box_is_not_there() {
    with_gpu!();
    let (mut shot, lights, boxes) = scene(Room::default());
    let with = Contents {
        lights: &lights,
        boxes: &boxes,
        ..Contents::default()
    };
    let without = Contents {
        lights: &lights,
        ..Contents::default()
    };
    let shaded = shot.frame(&view(), ON, &with);
    let bare = shot.frame(&view(), ON, &without);
    let off = shot.frame(&view(), OFF, &without);
    let (x, y) = (-1.4, -1.4);
    assert!(floor_level(&shot, &shaded, x, y) < 0.1 * floor_level(&shot, &off, x, y));
    // The same pixel with nothing in the light's way is the lit floor, to the level.
    assert_eq!(
        floor_level(&shot, &bare, x, y),
        floor_level(&shot, &off, x, y)
    );
    assert_eq!(
        bare, off,
        "with nothing to cast, shadows on draw the plain frame"
    );
}

#[test]
fn a_lit_floor_has_no_acne_and_the_shadow_has_no_stray_light() {
    with_gpu!();
    let (mut shot, lights, boxes) = scene(Room::default());
    let contents = Contents {
        lights: &lights,
        boxes: &boxes,
        ..Contents::default()
    };
    let (off, on) = (
        shot.frame(&view(), OFF, &contents),
        shot.frame(&view(), ON, &contents),
    );
    let cam = shot.camera(&view());
    // The screen box round the shadow's square, and a margin for its soft edge.
    let reach = SHADOW_HALF + 0.9;
    let corners = [
        (-reach, -reach),
        (reach, -reach),
        (-reach, reach),
        (reach, reach),
    ]
    .map(|(x, y)| pixel_of(&cam, v(x, y, FLOOR)));
    let (x0, x1) = (
        corners.iter().map(|c| c.0).min().unwrap(),
        corners.iter().map(|c| c.0).max().unwrap(),
    );
    let (y0, y1) = (
        corners.iter().map(|c| c.1).min().unwrap(),
        corners.iter().map(|c| c.1).max().unwrap(),
    );
    let mut outside = 0;
    let mut deviating = 0;
    for y in 0..SIZE.1 as usize {
        for x in 0..SIZE.0 as usize {
            if (x0..=x1).contains(&x) && (y0..=y1).contains(&y) {
                continue;
            }
            outside += 1;
            if (level_at(&off, (x, y)) - level_at(&on, (x, y))).abs() > 1.5 {
                deviating += 1;
            }
        }
    }
    println!("{deviating} of {outside} pixels outside the shadow deviate");
    assert_eq!(deviating, 0);
    // And inside the shadow's core, nothing but the box's own pixels is lit.
    let core = 1.2;
    let mut stray = 0;
    for step in 0..=10 {
        let t = -core + 2.0 * core * step as f32 / 10.0;
        for (x, y) in [(t, -core), (-core, t), (core, t)] {
            if floor_level(&shot, &on, x, y) > 0.1 * floor_level(&shot, &off, x, y) {
                stray += 1;
            }
        }
    }
    assert_eq!(stray, 0, "lit pixels in the shadow's core");
}

#[test]
fn light_does_not_pass_between_two_boxes_that_touch() {
    with_gpu!();
    let mut shot = Shot::new(&gpu_support::gpu().expect("a gpu"), room(Room::default()));
    // A light a little off the seam, so that every ray to the floor crosses it.
    let lights = [lamp(&shot.level, v(0.35, 0.0, 3.0))];
    let boxes = [box_at(v(-1.0, 0.0, -1.0)), box_at(v(1.0, 0.0, -1.0))];
    let contents = Contents {
        lights: &lights,
        boxes: &boxes,
        ..Contents::default()
    };
    let (off, on) = (
        shot.frame(&view(), OFF, &contents),
        shot.frame(&view(), ON, &contents),
    );
    keep("seam_on.png", &on);
    // The seam at x = 0 is crossed at the floor, in the shadow of both, where the
    // rays from the light slant to the side: sample along it in front of the boxes.
    for step in 0..=12 {
        let x = -1.3 + 2.0 * step as f32 / 12.0 * 1.3 - 0.0;
        let (lit, shaded) = (
            floor_level(&shot, &off, x, -1.5),
            floor_level(&shot, &on, x, -1.5),
        );
        assert!(shaded < 0.1 * lit, "x {x}: {shaded} of {lit}");
    }
}

#[test]
fn the_shadow_takes_only_the_dynamic_light_and_leaves_the_baked() {
    with_gpu!();
    let spec = Room {
        baked_lamp: Some((v(5.0, 5.0, 6.0), 0.4)),
        ..Room::default()
    };
    let (mut shot, lights, boxes) = scene(spec);
    let with_light = Contents {
        lights: &lights,
        boxes: &boxes,
        ..Contents::default()
    };
    let baked_only = Contents {
        boxes: &boxes,
        ..Contents::default()
    };
    let on = shot.frame(&view(), ON, &with_light);
    let baked = shot.frame(&view(), ON, &baked_only);
    let off = shot.frame(&view(), OFF, &with_light);
    keep("baked_on.png", &on);
    for (x, y) in [(-1.5, -1.5), (1.5, -1.5), (0.0, -1.7)] {
        let (a, b, c) = (
            floor_level(&shot, &on, x, y),
            floor_level(&shot, &baked, x, y),
            floor_level(&shot, &off, x, y),
        );
        assert!(b > 20.0, "the bake lights ({x}, {y}): {b}");
        assert!(
            (a - b).abs() <= 1.0,
            "inside the shadow only the bake is left at ({x}, {y}): {a} against {b}"
        );
        assert!(
            c > b + 10.0,
            "and the dynamic light adds to it with no shadows ({x}, {y}): {c} against {b}"
        );
    }
}

#[test]
fn the_edge_is_soft_and_steady_as_the_light_moves_by_less_than_a_texel() {
    with_gpu!();
    let (mut shot, mut lights, boxes) = scene(Room::default());
    shot.size = (960, 540);
    // A texel of the map where the box's top is, 3 from the light, is 3 * 2 / 512.
    let texel = 3.0 * 2.0 / 512.0;
    let cam = shot.camera(&view());
    let edge: Vec<(usize, usize)> = (-8..=8)
        .map(|i| pixel_of(&cam, v(i as f32 * 0.15, -SHADOW_HALF, FLOOR)))
        .collect();
    let (mut middle, mut spread) = (Vec::new(), Vec::new());
    for frame in 0..12 {
        let x = 0.25 * texel * frame as f32;
        lights[0] = lamp(&shot.level, v(x, 0.0, 3.0));
        let contents = Contents {
            lights: &lights,
            boxes: &boxes,
            ..Contents::default()
        };
        let image = shot.frame(&view(), ON, &contents);
        if frame == 0 {
            let png = mscent_render::png_encode::encode_png(960, 540, &image).expect("a png");
            if let Some(dir) = std::env::var_os("MSCENT_SHOTS") {
                std::fs::write(std::path::Path::new(&dir).join("edge.png"), png).expect("written");
            }
        }
        let at = |f: f32| {
            edge.iter()
                .map(|&e| crossing(&image, 960, e, f))
                .collect::<Vec<_>>()
        };
        let (lo, hi) = (at(0.1), at(0.9));
        middle.push(at(0.5));
        spread.push(lo.iter().zip(&hi).map(|(a, b)| b - a).collect());
    }
    let Stability { saw, crawl, width } = Stability::of(&middle, &spread);
    println!("edge: saw {saw:.3} px, crawl {crawl:.3} px, penumbra {width:.1} px");
    assert!(width > 6.0, "the edge is soft: {width} px");
    assert!(saw < 0.08, "the edge is straight: {saw} px");
    assert!(
        crawl < 0.02,
        "the edge stays put as the light moves: {crawl} px"
    );
}

/// Rows either side of the edge that `crossing` looks over, at the stability test's size.
const REACH: usize = 40;

/// Where down column `x` the floor crosses a given fraction of the way from the
/// shadow's dark to the light beyond its front edge, going down the screen, within
/// `REACH` rows of `near`, to a fraction of a row by the line between the pixels.
fn crossing(frame: &[u32], width: usize, (x, near): (usize, usize), fraction: f32) -> f32 {
    let rows: Vec<f32> = (near - REACH..=near + REACH)
        .map(|y| level_in(frame, width, (x, y)))
        .collect();
    let (dark, lit) = (rows[0], rows[rows.len() - 1]);
    let level = dark + fraction * (lit - dark);
    (0..rows.len() - 1)
        .find(|&i| rows[i] < level && rows[i + 1] >= level)
        .map_or(f32::NAN, |i| {
            (near - REACH + i) as f32 + (level - rows[i]) / (rows[i + 1] - rows[i])
        })
}

/// What the contours of a run say: `saw` the straight line's RMS deviation, `crawl`
/// how far each point of the contour moves over the run, and `width` the penumbra
/// from a tenth to nine tenths of the light.
struct Stability {
    saw: f32,
    crawl: f32,
    width: f32,
}

impl Stability {
    /// `middle` and `spread` are by frame, then by column: where the contour is at
    /// half the light, and the rows from a tenth to nine tenths.
    fn of(middle: &[Vec<f32>], spread: &[Vec<f32>]) -> Self {
        let frames = middle.len() as f32;
        let n = middle[0].len();
        let means: Vec<f32> = (0..n)
            .map(|i| middle.iter().map(|c| c[i]).sum::<f32>() / frames)
            .collect();
        let rms =
            |f: &dyn Fn(&Vec<f32>) -> f32| (middle.iter().map(f).sum::<f32>() / frames).sqrt();
        let crawl = rms(&|c| {
            c.iter()
                .zip(&means)
                .map(|(a, m)| (a - m) * (a - m))
                .sum::<f32>()
                / n as f32
        });
        let saw = rms(&|c| line_rms(c).powi(2));
        let width = spread.iter().flatten().sum::<f32>() / (frames * n as f32);
        Self { saw, crawl, width }
    }
}

/// RMS distance of the contour from its best straight line, by least squares.
fn line_rms(points: &[f32]) -> f32 {
    let n = points.len() as f32;
    let xs: Vec<f32> = (0..points.len()).map(|i| i as f32).collect();
    let (mx, my) = (xs.iter().sum::<f32>() / n, points.iter().sum::<f32>() / n);
    let sxx: f32 = xs.iter().map(|x| (x - mx) * (x - mx)).sum();
    let sxy: f32 = xs
        .iter()
        .zip(points)
        .map(|(x, y)| (x - mx) * (y - my))
        .sum();
    let slope = sxy / sxx;
    let r: f32 = xs
        .iter()
        .zip(points)
        .map(|(x, y)| (y - my - slope * (x - mx)).powi(2))
        .sum();
    (r / n).sqrt()
}

#[test]
fn an_actor_casts_a_shadow_and_so_does_a_mover() {
    with_gpu!();
    let spec = Room {
        slab: true,
        ..Room::default()
    };
    let (mut shot, lights, _) = scene(spec);
    let actors = [Actor {
        kind: ActorKind::Crate,
        pos: v(-4.0, 0.0, FLOOR + 1.0),
        yaw: 0.0,
    }];
    let moved = [MoverPose {
        offset: v(4.0, 0.0, 0.0),
        ..MoverPose::AUTHORED
    }];
    let plain = Contents {
        lights: &lights,
        ..Contents::default()
    };
    let contents = Contents {
        lights: &lights,
        actors: &actors,
        poses: &moved,
        ..Contents::default()
    };
    let (off, on) = (
        shot.frame(&view(), OFF, &contents),
        shot.frame(&view(), ON, &contents),
    );
    keep("actor_mover_on.png", &on);
    keep("actor_mover_off.png", &off);
    let bare = shot.frame(&view(), ON, &plain);
    let changed = |a: &[u32], b: &[u32]| a.iter().zip(b).filter(|(p, q)| p != q).count();
    println!(
        "shadows change {} pixels of a frame with an actor and a mover",
        changed(&off, &on)
    );
    assert!(changed(&off, &on) > 300);
    // The mover moved by 4 casts a shadow from 7 to 11.7 along the floor.
    let slab = floor_level(&shot, &on, 9.0, -1.0);
    let before = floor_level(&shot, &off, 9.0, -1.0);
    assert!(
        slab < 0.2 * before,
        "the moved mover's shadow: {slab} of {before}"
    );
    // And with it left out of the frame the pixel is the lit floor again.
    assert!(floor_level(&shot, &bare, 9.0, -1.0) > 0.8 * before);
}
/// Lights and casters a level or a script could hand the renderer: none of it may
/// panic or raise a GPU error, and a frame still comes out.
#[test]
fn hostile_lights_and_casters_are_ignored_and_never_panic() {
    with_gpu!();
    let (mut shot, lights, boxes) = scene(Room::default());
    let place = |shot: &Shot, pos: Vec3, radius: f32, color: Vec3| {
        let mut light = mscent_render::DynLight::new();
        let placed = light.place(shot.level.scene(), pos, radius, color);
        (light, placed)
    };
    let white = v(1.5, 1.5, 1.5);
    // A light that is not a place, a light of no radius and one of no colour are
    // refused by `place`, and what is left of them is dark.
    let (nan, nan_placed) = place(&shot, v(f32::NAN, 0.0, 3.0), 16.0, white);
    let (flat, flat_placed) = place(&shot, v(0.0, 0.0, 3.0), 0.0, white);
    let (black, _) = place(&shot, v(0.0, 0.0, 3.0), 16.0, v(0.0, 0.0, 0.0));
    assert!(!nan_placed && !flat_placed);
    // A light inside the box, one on a wall, one at the room's far corner, one of
    // the biggest radius, and more lights than a frame holds.
    let (inside, _) = place(&shot, v(0.0, 0.0, -1.0), 16.0, white);
    let (on_wall, _) = place(&shot, v(0.0, shade::HALF as f32 - 0.001, 0.0), 16.0, white);
    let (corner, _) = place(&shot, v(11.9, 11.9, 7.9), 16.0, white);
    let (huge, _) = place(&shot, v(0.0, 0.0, 3.0), 1.0e9, white);
    let crowd = [
        nan,
        flat,
        black,
        inside,
        on_wall,
        corner,
        huge,
        lights[0].clone(),
    ];
    for pick in 0..crowd.len() {
        for lights in [&crowd[pick..], &crowd[..=pick], &crowd[pick..=pick]] {
            let contents = Contents {
                lights,
                boxes: &boxes,
                ..Contents::default()
            };
            let frame = shot.frame(&view(), ON, &contents);
            assert_eq!(frame.len(), (SIZE.0 * SIZE.1) as usize);
        }
    }
    // Shadows on with no light, no casters and no models at all.
    let empty = Contents::default();
    assert_eq!(
        shot.frame(&view(), ON, &empty),
        shot.frame(&view(), OFF, &empty)
    );
    // A box far outside the light's reach, and one the size of the room.
    let far = [box_at(v(500.0, 500.0, 500.0)), {
        let mut big = box_at(v(0.0, 0.0, 0.0));
        big.scale = 20.0;
        big
    }];
    let contents = Contents {
        lights: &lights,
        boxes: &far,
        ..Contents::default()
    };
    shot.frame(&view(), ON, &contents);
}
