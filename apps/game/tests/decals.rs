//! Scorch decals on real pixels: a real baked level, the real depth buffer and
//! the real band renderer.

use miniscent_game::assets;
use mscent_game::fx::budget::cost_of;
use mscent_game::fx::combat::{CombatFx, Impact, ShotFx};
use mscent_game::fx::decal::FaceOutline;
use mscent_math::{Vec3, Vec3d};
use mscent_render::particles::{ParticleDraws, SpriteBank};
use mscent_render::{BuildConfig, Framebuffer, LevelRenderer, Quality, View};
use mscent_world::materials::BRICK;
use mscent_world::{Brush, Entity, Level, Op};

const W: u32 = 480;
const H: u32 = 270;
const FRAME: u64 = 16_666_667;
/// On the pillar's face x = 0, 0.2 from its edge at y = 4. The edge is
/// convex, so an unclipped square would hang in the air beside the pillar.
const MARK_AT: Vec3 = Vec3::new(0.0, 3.8, 0.0);
const WALL_NORMAL: Vec3 = Vec3::new(-1.0, 0.0, 0.0);
/// The mark's half-side for brick, from `surface_mark`.
const MARK_RADIUS: f32 = 0.38;

fn v(x: f32, y: f32, z: f32) -> Vec3 {
    Vec3::new(x, y, z)
}

/// A lit room of half-side 20 with a square pillar from floor to ceiling;
/// `divider` adds a slab at x in `[-6, -4]` that walls off the pillar's face.
fn renderer(divider: bool) -> LevelRenderer {
    let d = |x, y, z| Vec3d::new(x, y, z);
    let cuboid = |a, b, op, order| Brush::cuboid(a, b, op, order).expect("a brush");
    let mut brushes = vec![cuboid(
        d(-20.0, -20.0, -20.0),
        d(20.0, 20.0, 20.0),
        Op::Subtract,
        0,
    )];
    brushes.push(cuboid(d(0.0, -4.0, -21.0), d(4.0, 4.0, 21.0), Op::Add, 1));
    if divider {
        brushes.push(cuboid(
            d(-6.0, -21.0, -21.0),
            d(-4.0, 21.0, 21.0),
            Op::Add,
            2,
        ));
    }
    let level = Level {
        brushes,
        entities: vec![Entity::Light {
            pos: v(-12.0, 0.0, 5.0),
            color: v(1.0, 1.0, 1.0),
            radius: 60.0,
            intensity: 1.0,
        }],
    };
    let cfg = BuildConfig {
        luxel_size: 1.0,
        shadow_samples: 1,
        // Clear air, so a mark across the room is not lost in the haze.
        fog_start: 200.0,
        fog_end: 400.0,
        ..BuildConfig::default()
    };
    let materials = assets::load_materials(None).expect("textures");
    LevelRenderer::from_level(&level, materials, &cfg).expect("a level")
}

/// A view from `eye` toward `target` (yaw turns left from +Y).
fn look(eye: Vec3, target: Vec3) -> View {
    let d = target - eye;
    let yaw = (-d.x).atan2(d.y);
    let pitch = d.z.atan2(d.x.hypot(d.y));
    View::from_angles(eye, yaw, pitch, 0.0)
}

/// The settled mark of one brick shot at `MARK_AT`: the bolt has arrived and
/// every spark, chip and puff has died, so only the decal is left.
fn settled(r: &LevelRenderer, clipped: bool) -> CombatFx {
    let mut fx = CombatFx::new();
    let shot = ShotFx {
        by_player: true,
        muzzle: v(0.0, 0.0, 0.0),
        end: MARK_AT,
        normal: WALL_NORMAL,
        impact: Impact::Surface {
            material: BRICK,
            tint: 0xff90_3020,
        },
    };
    let outline = r
        .scene()
        .face_at(MARK_AT, WALL_NORMAL)
        .and_then(FaceOutline::of);
    assert!(outline.is_some(), "the wall's face is found");
    fx.shot_on(&shot, outline.as_ref().filter(|_| clipped));
    for _ in 0..240 {
        fx.advance(FRAME, r.scene().bsp());
    }
    assert_eq!((fx.live(), fx.bolts(), fx.decals()), (0, 0, 1));
    fx
}

fn frame(r: &LevelRenderer, view: &View, fx: Option<&mut CombatFx>) -> (Framebuffer, usize) {
    let mut fb = Framebuffer::new(W, H).expect("a target");
    let mut draws = ParticleDraws::default();
    if let Some(fx) = fx {
        let cam = r.camera(view, W, H).expect("a camera");
        fx.collect(&cam, r.fog(), None, &mut draws);
    }
    let bank = SpriteBank::new();
    let given = Some((&draws, &bank));
    r.render_with_particles(&mut fb, view, Quality::Low, &[], None, given)
        .expect("a frame");
    (fb, draws.decals.len())
}

fn changed(a: &Framebuffer, b: &Framebuffer) -> Vec<(i32, i32)> {
    let (pa, pb) = (a.pixels(), b.pixels());
    (0..W as usize * H as usize)
        .filter(|&i| pa[i] != pb[i])
        .map(|i| ((i % W as usize) as i32, (i / W as usize) as i32))
        .collect()
}

/// The world point a pixel shows, from the view and the stored depth.
fn world_at(r: &LevelRenderer, view: &View, fb: &Framebuffer, (x, y): (i32, i32)) -> Vec3 {
    let cam = r.camera(view, W, H).expect("a camera");
    let depth = 1.0 / fb.depth_at(x, y).expect("on screen");
    let f = cam.focal();
    let cx = (x as f32 + 0.5 - W as f32 * 0.5) / f * depth;
    let cy = -(y as f32 + 0.5 - H as f32 * 0.5) / f * depth;
    view.pos + view.orientation.rotate(v(cx, cy, -depth))
}

/// Views of the mark from far and near, face on and at a graze.
fn views() -> Vec<View> {
    [
        v(-6.0, 6.0, 0.0),
        v(-10.0, 2.0, 0.0),
        v(-5.0, 10.0, 5.0),
        v(-12.0, 12.0, -6.0),
        v(-1.5, 8.0, 2.0),
        v(-15.0, 0.0, 4.0),
    ]
    .iter()
    .map(|&eye| look(eye, MARK_AT))
    .collect()
}

#[test]
fn a_mark_stays_on_its_face_from_every_angle() {
    let r = renderer(false);
    let (mut cut, mut spilling) = (settled(&r, true), settled(&r, false));
    let mut spilled_somewhere = false;
    let mut centres = Vec::new();
    for (i, view) in views().iter().enumerate() {
        let (base, _) = frame(&r, view, None);
        let (marked, listed) = frame(&r, view, Some(&mut cut));
        let hit = changed(&base, &marked);
        assert_eq!(listed, 1, "view {i}: the decal is in the draw list");
        assert!(hit.len() >= 15, "view {i}: only {} pixels", hit.len());
        let mut sum = Vec3::ZERO;
        for &px in &hit {
            let p = world_at(&r, view, &marked, px);
            // On the pillar's face x = 0 and inside both it and the square.
            assert!(p.x.abs() < 0.01, "view {i}: {px:?} is at {p:?}");
            assert!(
                p.y <= 4.001,
                "view {i}: {px:?} went past the edge to y = {}",
                p.y
            );
            let (dy, dz) = ((p.y - MARK_AT.y).abs(), (p.z - MARK_AT.z).abs());
            let reach = MARK_RADIUS + 0.01;
            assert!(dy <= reach && dz <= reach, "view {i}: {px:?} is at {p:?}");
            sum = sum + p;
        }
        centres.push(sum * (1.0 / hit.len() as f32));
        // The unclipped square on the same level hangs in the air past the edge,
        // drawn over whatever is behind it.
        let (wide, _) = frame(&r, view, Some(&mut spilling));
        spilled_somewhere |= changed(&base, &wide)
            .iter()
            .any(|&px| world_at(&r, view, &wide, px).x.abs() > 0.05);
    }
    assert!(
        spilled_somewhere,
        "without the cut the mark hangs past the edge"
    );
    // Whatever the angle, the pixels average to the same place on the wall.
    let first = centres[0];
    for c in &centres {
        assert!((*c - first).length() < 0.15, "{c:?} against {first:?}");
    }
}

/// How many pixels flip off for one frame between two frames that have them
/// on, or on for one frame between two that do not.
fn flickers(sets: &[Vec<bool>]) -> usize {
    let flipped = |((&a, &b), &c): ((&bool, &bool), &bool)| (a && c && !b) || (!a && !c && b);
    sets.windows(3)
        .map(|w| {
            let pixels = w[0].iter().zip(&w[1]).zip(&w[2]);
            pixels.filter(|&p| flipped(p)).count()
        })
        .sum()
}

fn marked_pixels(r: &LevelRenderer, fx: &mut CombatFx, view: &View) -> Vec<bool> {
    let (base, _) = frame(r, view, None);
    let (marked, _) = frame(r, view, Some(fx));
    let mut on = vec![false; W as usize * H as usize];
    for (x, y) in changed(&base, &marked) {
        on[y as usize * W as usize + x as usize] = true;
    }
    on
}

#[test]
fn a_mark_does_not_flicker_between_adjacent_camera_positions() {
    let r = renderer(false);
    let mut fx = settled(&r, true);
    // Two sweeps in steps of a hundredth of a unit: sideways, which slides the
    // mark across the screen, and straight in, which grows it; and one at a
    // graze, where depth changes fastest across the mark.
    let sweeps: [(Vec3, Vec3); 3] = [
        (v(-8.0, 6.0, 0.0), v(0.0, 0.0, 0.01)),
        (v(-10.0, 10.0, 1.0), v(0.007, -0.007, 0.0)),
        (v(-2.0, 9.0, 1.0), v(0.0, -0.01, 0.0)),
    ];
    for (k, (start, step)) in sweeps.into_iter().enumerate() {
        let sets: Vec<Vec<bool>> = (0..40)
            .map(|i| {
                let eye = start + step * i as f32;
                marked_pixels(&r, &mut fx, &look(eye, MARK_AT))
            })
            .collect();
        let sizes: Vec<usize> = sets
            .iter()
            .map(|s| s.iter().filter(|&&p| p).count())
            .collect();
        let (lo, hi) = (sizes.iter().min().unwrap(), sizes.iter().max().unwrap());
        assert!(*lo >= 15, "sweep {k}: only {lo} pixels at the smallest");
        let worst = sizes.windows(2).map(|w| w[0].abs_diff(w[1])).max().unwrap();
        assert!(
            worst * 10 <= *hi,
            "sweep {k}: the size jumps by {worst} of {hi}"
        );
        let flips = flickers(&sets);
        let total: usize = sizes.iter().sum();
        assert!(
            flips * 200 <= total,
            "sweep {k}: {flips} flips in {total} pixels"
        );
    }
}

#[test]
fn a_mark_behind_a_wall_changes_no_pixel_and_in_the_open_it_does() {
    // The eye is on the near side of the slab, the mark on the pillar beyond.
    let walled = renderer(true);
    let mut fx = settled(&walled, true);
    let view = look(v(-10.0, 6.0, 0.0), MARK_AT);
    let (base, _) = frame(&walled, &view, None);
    let (with, listed) = frame(&walled, &view, Some(&mut fx));
    assert_eq!(
        listed, 1,
        "the mark is submitted: it faces the eye and is on screen"
    );
    assert!(
        changed(&base, &with).is_empty(),
        "and the depth test hides it"
    );

    let open = renderer(false);
    let mut fx = settled(&open, true);
    let (base, _) = frame(&open, &view, None);
    let (with, _) = frame(&open, &view, Some(&mut fx));
    assert!(
        changed(&base, &with).len() >= 15,
        "the same mark in the open shows"
    );
}

#[test]
fn one_worker_and_many_give_identical_pixels() {
    let mut r = renderer(false);
    let mut fx = settled(&r, true);
    for view in views() {
        let mut shots = Vec::new();
        for (rows, workers) in [(24, 1), (24, 4), (7, 3), (1, 8)] {
            r.set_tiling(rows, workers);
            shots.push(frame(&r, &view, Some(&mut fx)).0.pixels().to_vec());
        }
        assert!(shots.windows(2).all(|w| w[0] == w[1]), "bands disagree");
    }
}

#[test]
fn marks_are_charged_to_the_budget_by_their_area() {
    let r = renderer(false);
    let mut fx = settled(&r, true);
    let cam = |eye| {
        let view = look(eye, MARK_AT);
        r.camera(&view, W, H).expect("a camera")
    };
    let mut cost = |eye| {
        let mut draws = ParticleDraws::default();
        fx.collect(&cam(eye), r.fog(), None, &mut draws);
        cost_of(&draws, None, W as f32, H as f32)
    };
    let near = cost(v(-3.0, 7.0, 1.0));
    let far = cost(v(-15.0, 12.0, 0.0));
    assert!(near > far && far > 0.0, "near {near}, far {far}");
}
