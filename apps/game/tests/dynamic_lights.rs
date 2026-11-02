//! Dynamic lights on real pixels: through a baked level, its walls and its
//! enemy meshes, and from the combat effects that spawn them.

use miniscent_game::assets;
use mscent_game::fx::combat::{CombatFx, DRONE_TINT, Impact, ShotFx};
use mscent_game::fx::lights::LightSpec;
use mscent_math::{Vec3, Vec3d};
use mscent_render::particles::{ParticleDraws, SpriteBank};
use mscent_render::{
    Actor, ActorKind, BuildConfig, DynLight, Effects, Framebuffer, LevelRenderer, Quality, View,
};
use mscent_world::materials::METAL;
use mscent_world::{Brush, Entity, Level, Op};

const W: u32 = 320;
const H: u32 = 180;
const FRAME: u64 = 16_666_667;

fn v(x: f32, y: f32, z: f32) -> Vec3 {
    Vec3::new(x, y, z)
}

/// A lit room; `divider` adds a slab at x in `[-1, 1]` that fills its section.
fn renderer(divider: bool) -> LevelRenderer {
    let d = |x, y, z| Vec3d::new(x, y, z);
    let cuboid = |a, b, op, order| Brush::cuboid(a, b, op, order).expect("a brush");
    let mut brushes = vec![cuboid(
        d(-20.0, -20.0, -20.0),
        d(20.0, 20.0, 20.0),
        Op::Subtract,
        0,
    )];
    if divider {
        brushes.push(cuboid(
            d(-1.0, -21.0, -21.0),
            d(1.0, 21.0, 21.0),
            Op::Add,
            1,
        ));
    }
    let level = Level {
        brushes,
        entities: vec![Entity::Light {
            pos: v(-5.0, 0.0, 5.0),
            color: v(1.0, 1.0, 1.0),
            radius: 40.0,
            intensity: 1.0,
        }],
        labels: Vec::new(),
        mover_groups: Vec::new(),
        volumes: Vec::new(),
    };
    let cfg = BuildConfig {
        luxel_size: 1.0,
        shadow_samples: 1,
        ..BuildConfig::default()
    };
    let materials = assets::load_materials(None).expect("textures");
    LevelRenderer::from_level(&level, materials, &cfg).expect("a level")
}

/// Looking along +X from `x`.
fn view_from(x: f32) -> View {
    View::from_angles(v(x, 0.0, 0.0), -core::f32::consts::FRAC_PI_2, 0.0, 0.0)
}

fn placed(r: &LevelRenderer, pos: Vec3, radius: f32, color: f32) -> DynLight {
    let mut light = DynLight::new();
    assert!(light.place(r.scene(), pos, radius, v(color, color, color)));
    light
}

fn enemy_at(pos: Vec3) -> Actor {
    Actor {
        kind: ActorKind::Enemy,
        pos,
        yaw: 0.0,
    }
}

fn frame(r: &LevelRenderer, view: &View, actors: &[Actor], lights: &[DynLight]) -> Framebuffer {
    let mut fb = Framebuffer::new(W, H).expect("a target");
    let effects = Effects {
        particles: None,
        lights,
        poses: &[],
        ..Effects::default()
    };
    r.render_with_effects(&mut fb, view, Quality::High, actors, None, &effects)
        .expect("a frame");
    fb
}

fn changed(a: &Framebuffer, b: &Framebuffer) -> usize {
    let pairs = a.pixels().iter().zip(b.pixels());
    pairs.filter(|(x, y)| x != y).count()
}

/// Sum of the colour channels over the pixels of a centred box `w` x `h`.
fn brightness(fb: &Framebuffer, w: u32, h: u32) -> u64 {
    let (x0, y0) = ((W - w) / 2, (H - h) / 2);
    let mut sum = 0u64;
    for y in y0..y0 + h {
        for x in x0..x0 + w {
            let p = fb.pixels()[(y * W + x) as usize];
            sum += u64::from(p >> 16 & 255) + u64::from(p >> 8 & 255) + u64::from(p & 255);
        }
    }
    sum
}

#[test]
fn a_light_in_the_room_brightens_the_wall_facing_it() {
    let r = renderer(false);
    let view = view_from(10.0);
    let dark = frame(&r, &view, &[], &[]);
    let light = placed(&r, v(17.0, 0.0, 0.0), 8.0, 1.2);
    let lit = frame(&r, &view, &[], std::slice::from_ref(&light));
    let (before, after) = (brightness(&dark, 100, 60), brightness(&lit, 100, 60));
    assert!(after > before + before / 4, "{before} became {after}");
    // The wall behind the viewer is 30 units from the light, past its radius.
    let behind = View::from_angles(v(10.0, 0.0, 0.0), core::f32::consts::FRAC_PI_2, 0.0, 0.0);
    let away = changed(
        &frame(&r, &behind, &[], &[]),
        &frame(&r, &behind, &[], &[light]),
    );
    assert_eq!(away, 0, "a wall out of reach is not touched");
}

#[test]
fn a_wall_behind_a_wall_stays_unlit_by_exact_pixels() {
    let r = renderer(true);
    let view = view_from(-4.0);
    let bare = frame(&r, &view, &[], &[]);
    // A big flash just past the slab, which the camera looks straight at.
    let beyond = placed(&r, v(3.0, 0.0, 0.0), 30.0, 3.0);
    assert_eq!(changed(&bare, &frame(&r, &view, &[], &[beyond])), 0);
    // The same flash on the camera's side shows, so the zero is the wall's doing.
    let here = placed(&r, v(-6.0, 0.0, 0.0), 30.0, 3.0);
    let seen = changed(&bare, &frame(&r, &view, &[], &[here]));
    assert!(seen > 10_000, "{seen} of {} pixels", W * H);
    // And with no slab, the flash from x = 3 reaches the room.
    let open = renderer(false);
    let beyond = placed(&open, v(3.0, 0.0, 0.0), 30.0, 3.0);
    let was = frame(&open, &view, &[], &[]);
    assert!(changed(&was, &frame(&open, &view, &[], &[beyond])) > 10_000);
}

#[test]
fn an_enemy_mesh_is_lit_and_only_the_mesh() {
    let r = renderer(false);
    let view = view_from(-4.0);
    let actors = [enemy_at(v(3.0, 0.0, 0.0))];
    let dark = frame(&r, &view, &actors, &[]);
    // Small enough to stay off the walls, which are 17 or more units away.
    let light = placed(&r, v(3.0, -1.5, 0.0), 4.0, 1.5);
    let lit = frame(&r, &view, &actors, &[light]);
    let n = changed(&dark, &lit);
    assert!(n > 100, "the enemy's {n} pixels changed");
    assert!(n < (W * H / 4) as usize, "and not much else: {n}");
    // Every change is inside a box around the mesh: 1.1 units of drone is a
    // tenth of the frame at 7 units.
    let w = W as usize;
    let moved = |p: usize| dark.pixels()[p] != lit.pixels()[p];
    let (cx, cy) = (w / 2, H as usize / 2);
    for p in (0..dark.pixels().len()).filter(|&p| moved(p)) {
        let (x, y) = (p % w, p / w);
        assert!(x.abs_diff(cx) < 60 && y.abs_diff(cy) < 45, "({x}, {y})");
    }
    let before = brightness(&dark, 80, 60);
    assert!(brightness(&lit, 80, 60) > before, "it got brighter");
}

#[test]
fn an_enemy_behind_a_wall_from_the_light_is_not_lit() {
    let r = renderer(true);
    let view = View::from_angles(v(-8.0, -6.0, 0.0), -0.5, 0.0, 0.0);
    // On the camera's side of the slab, in view, with the flash on the other.
    let actors = [enemy_at(v(-4.0, 0.0, 0.0))];
    let bare = frame(&r, &view, &actors, &[]);
    let beyond = placed(&r, v(3.0, 0.0, 0.0), 30.0, 3.0);
    assert_eq!(changed(&bare, &frame(&r, &view, &actors, &[beyond])), 0);
    let near = placed(&r, v(-4.0, 1.5, 0.0), 6.0, 3.0);
    assert!(changed(&bare, &frame(&r, &view, &actors, &[near])) > 100);
}

#[test]
fn a_light_that_has_faded_out_or_never_lit_changes_no_pixel() {
    let r = renderer(false);
    let view = view_from(-4.0);
    let actors = [enemy_at(v(3.0, 0.0, 0.0))];
    let bare = frame(&r, &view, &actors, &[]);
    let mut out = placed(&r, v(3.0, 0.0, 0.0), 10.0, 1.0);
    assert!(out.set_color(Vec3::ZERO));
    let lights = [DynLight::new(), out];
    assert_eq!(changed(&bare, &frame(&r, &view, &actors, &lights)), 0);
    // The old entry point is the same frame.
    let mut old = Framebuffer::new(W, H).expect("a target");
    r.render_with_particles(&mut old, &view, Quality::High, &actors, None, None)
        .expect("a frame");
    assert_eq!(old.pixels(), bare.pixels());
}

#[test]
fn hostile_lights_do_not_panic_and_light_nothing() {
    let r = renderer(true);
    let view = view_from(-4.0);
    let actors = [enemy_at(v(-2.0, 0.0, 0.0)), enemy_at(v(f32::NAN, 0.0, 0.0))];
    let bare = frame(&r, &view, &actors, &[]);
    let nan = f32::NAN;
    let inf = f32::INFINITY;
    let mut fx = CombatFx::new();
    for (pos, radius, color) in [
        (v(nan, 0.0, 0.0), 5.0, v(1.0, 1.0, 1.0)),
        (v(0.0, 0.0, 0.0), inf, v(1.0, 1.0, 1.0)),
        (v(-4.0, 0.0, 0.0), -5.0, v(1.0, 1.0, 1.0)),
        (v(-4.0, 0.0, 0.0), 5.0, v(nan, 1.0, 1.0)),
        (v(-4.0, 0.0, 0.0), 5.0, v(-1.0, 1.0, 1.0)),
        (v(1.0e30, 1.0e30, 0.0), 5.0, v(1.0, 1.0, 1.0)),
    ] {
        fx.light(LightSpec {
            pos,
            radius,
            color,
            life: 1.0,
            lift: Vec3::ZERO,
        });
    }
    assert_eq!(fx.live_lights(), 1, "only the one far away is valid");
    assert_eq!(
        changed(&bare, &frame(&r, &view, &actors, fx.lights(r.scene()))),
        0
    );
    // A light buried in the slab and one at the world limit reach nothing.
    let buried = placed(&r, v(0.0, 0.0, 0.0), 10.0, 5.0);
    let mut huge = DynLight::new();
    assert!(huge.place(
        r.scene(),
        v(-4.0, 0.0, 0.0),
        1.0e30,
        v(1.0e30, 1.0e30, 1.0e30)
    ));
    assert!(!DynLight::new().place(r.scene(), v(nan, nan, nan), nan, v(nan, nan, nan)));
    // More lights than there are slots: the extra are ignored, not a crash.
    let many: Vec<DynLight> = (0..9).map(|_| buried.clone()).chain([huge]).collect();
    frame(&r, &view, &actors, &many);
}

#[test]
fn one_worker_and_many_give_identical_pixels() {
    let view = View::from_angles(v(-6.0, -4.0, 1.0), -0.5, 0.0, 0.0);
    let actors = [enemy_at(v(-2.0, 0.0, 0.0)), enemy_at(v(-3.0, 2.0, 1.0))];
    let mut r = renderer(true);
    let mut fx = CombatFx::new();
    fx.drone_explosion(v(-2.0, 0.5, 0.0), DRONE_TINT);
    fx.advance(FRAME, r.scene().bsp());
    let mut draws = ParticleDraws::default();
    let cam = r.camera(&view, W, H).expect("a camera");
    fx.collect(&cam, r.fog(), None, &mut draws);
    assert!(!draws.is_empty());
    let bank = SpriteBank::new();
    let lights = fx.lights(r.scene()).to_vec();
    assert!(lights.iter().any(DynLight::is_lit));
    let mut draw = |workers: usize| {
        r.set_tiling(24, workers);
        let mut fb = Framebuffer::new(W, H).expect("a target");
        let effects = Effects {
            particles: Some((&draws, &bank)),
            lights: &lights,
            poses: &[],
            ..Effects::default()
        };
        r.render_with_effects(&mut fb, &view, Quality::High, &actors, None, &effects)
            .expect("a frame");
        fb.pixels().to_vec()
    };
    let one = draw(1);
    for workers in [2, 3, 8] {
        assert!(one == draw(workers), "{workers} workers differ from one");
    }
}

fn arrive(fx: &mut CombatFx, r: &LevelRenderer, impact: Impact) {
    fx.shot(&ShotFx {
        by_player: true,
        muzzle: v(-3.5, -0.2, -0.2),
        end: v(19.9, 0.0, 0.0),
        normal: v(-1.0, 0.0, 0.0),
        impact,
    });
    assert_eq!(fx.live_lights(), 0, "nothing lights before the bolt lands");
    for _ in 0..8 {
        fx.advance(FRAME, r.scene().bsp());
    }
    assert_eq!(fx.bolts(), 0);
}

#[test]
fn the_recipes_spawn_the_lights_of_the_design() {
    let r = renderer(false);
    let mut fx = CombatFx::new();
    fx.drone_explosion(v(5.0, 0.0, 0.0), DRONE_TINT);
    let lights = fx.lights(r.scene());
    assert_eq!(lights.len(), 1);
    assert_eq!(lights[0].radius(), 12.0);
    // 0.25 s is 15 frames of the 60 Hz clock.
    for _ in 0..14 {
        fx.advance(FRAME, r.scene().bsp());
    }
    assert_eq!(fx.live_lights(), 1);
    fx.advance(FRAME, r.scene().bsp());
    assert_eq!(fx.live_lights(), 0);

    let hit = |impact| {
        let mut fx = CombatFx::new();
        arrive(&mut fx, &r, impact);
        let lights = fx.lights(r.scene());
        assert_eq!(lights.len(), 1);
        (lights[0].radius(), lights[0].pos())
    };
    let surface = hit(Impact::Surface {
        material: METAL,
        tint: 0xff80_8080,
    });
    assert_eq!(surface.0, 4.0);
    assert!(
        (surface.1.x - 19.3).abs() < 0.1,
        "off the wall: {:?}",
        surface.1
    );
    assert_eq!(hit(Impact::Drone).0, 4.0);
    // 0.1 s: gone after 6 frames of the particle clock.
    let mut fx = CombatFx::new();
    arrive(&mut fx, &r, Impact::Drone);
    assert!(fx.live_lights() <= 1);
    for _ in 0..7 {
        fx.advance(FRAME, r.scene().bsp());
    }
    assert_eq!(fx.live_lights(), 0);
    let mut none = CombatFx::new();
    arrive(&mut none, &r, Impact::None);
    assert_eq!(
        none.live_lights(),
        0,
        "a shot into the distance has no flash"
    );
}

#[test]
fn a_quality_allows_the_lights_its_row_says() {
    let r = renderer(false);
    for (quality, allowed) in [(Quality::High, 4), (Quality::Medium, 2), (Quality::Low, 1)] {
        let mut fx = CombatFx::new();
        fx.set_quality(quality);
        for i in 0..8 {
            fx.drone_explosion(v(i as f32, 0.0, 0.0), DRONE_TINT);
        }
        assert_eq!(fx.lights(r.scene()).len(), allowed, "{quality:?}");
    }
}
