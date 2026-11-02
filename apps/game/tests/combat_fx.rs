//! Combat effects on real pixels: through a real baked level, against the
//! real depth buffer, and through the whole session.

use std::time::Duration;

use miniscent_game::app::{RunOptions, run};
use miniscent_game::{GameSession, SessionConfig, assets, levels};
use mscent_game::fx::budget::cost_of;
use mscent_game::fx::combat::{CombatFx, DRONE_TINT, Impact, ShotFx, enemy_muzzle};
use mscent_math::{Vec3, Vec3d};
use mscent_platform::{HeadlessPlatform, Script};
use mscent_render::particles::{ParticleDraws, SpriteBank};
use mscent_render::{Actor, ActorKind, BuildConfig, Framebuffer, LevelRenderer, Quality, View};
use mscent_world::materials::{BRICK, METAL};
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
    };
    let cfg = BuildConfig {
        luxel_size: 1.0,
        shadow_samples: 1,
        ..BuildConfig::default()
    };
    let materials = assets::load_materials(None).expect("textures");
    LevelRenderer::from_level(&level, materials, &cfg).expect("a level")
}

/// On the near side of the divider, looking along +X at it.
fn view() -> View {
    View::from_angles(v(-4.0, 0.0, 0.0), -core::f32::consts::FRAC_PI_2, 0.0, 0.0)
}

fn draws_of(r: &LevelRenderer, fx: Option<&mut CombatFx>) -> ParticleDraws {
    let mut draws = ParticleDraws::default();
    if let Some(fx) = fx {
        let cam = r.camera(&view(), W, H).expect("a camera");
        fx.collect(&cam, r.fog(), None, &mut draws);
    }
    draws
}

fn drawn(r: &LevelRenderer, actors: &[Actor], draws: &ParticleDraws) -> Framebuffer {
    let mut fb = Framebuffer::new(W, H).expect("a target");
    let bank = SpriteBank::new();
    r.render_with_particles(
        &mut fb,
        &view(),
        Quality::Low,
        actors,
        None,
        Some((draws, &bank)),
    )
    .expect("a frame");
    fb
}

fn frame(r: &LevelRenderer, fx: Option<&mut CombatFx>) -> Framebuffer {
    drawn(r, &[], &draws_of(r, fx))
}

fn changed(a: &Framebuffer, b: &Framebuffer) -> usize {
    let same = a.pixels().iter().zip(b.pixels());
    same.filter(|(x, y)| x != y).count()
}

/// Everything on the far side of the divider: an explosion, and a bolt still
/// on its way from x = 3 to x = 12.
fn far_side_fx(r: &LevelRenderer) -> CombatFx {
    let mut fx = CombatFx::new();
    fx.drone_explosion(v(5.0, 0.0, 0.0), DRONE_TINT);
    fx.shot(&ShotFx {
        by_player: false,
        muzzle: v(3.0, -1.0, 0.0),
        end: v(12.0, -1.0, 0.0),
        normal: v(-1.0, 0.0, 0.0),
        impact: Impact::Surface {
            material: METAL,
            tint: 0xff80_8080,
        },
    });
    fx.advance(FRAME, r.scene().bsp());
    fx
}

#[test]
fn a_shot_behind_a_wall_draws_no_pixels_and_in_the_open_it_does() {
    let walled = renderer(true);
    let mut fx = far_side_fx(&walled);
    assert!(fx.bolts() > 0 && fx.live() > 0, "something to hide");
    let hidden = changed(&frame(&walled, None), &frame(&walled, Some(&mut fx)));
    assert_eq!(hidden, 0, "the old tracer showed through walls");

    let open = renderer(false);
    let mut fx = far_side_fx(&open);
    let seen = changed(&frame(&open, None), &frame(&open, Some(&mut fx)));
    assert!(seen > 100, "the same effects in the open: {seen} pixels");
}

#[test]
fn a_bolt_alone_shows_in_the_open_and_not_behind_a_wall() {
    let bolt_pixels = |r: &LevelRenderer| {
        let mut fx = CombatFx::new();
        fx.shot(&ShotFx {
            by_player: false,
            muzzle: v(3.0, -1.0, 0.0),
            end: v(12.0, -1.0, 0.0),
            normal: v(-1.0, 0.0, 0.0),
            impact: Impact::None,
        });
        let mut draws = draws_of(r, Some(&mut fx));
        // Only the streaks: the muzzle flash and the head's glow are sprites.
        assert_eq!(draws.streaks.len(), 2, "core and edge");
        draws.over.clear();
        draws.add.clear();
        changed(&frame(r, None), &drawn(r, &[], &draws))
    };
    assert_eq!(bolt_pixels(&renderer(true)), 0);
    assert!(bolt_pixels(&renderer(false)) > 10);
}

#[test]
fn an_enemy_muzzle_flash_shows_in_front_of_its_own_hull() {
    let r = renderer(false);
    let at = v(5.0, 0.0, 0.0);
    let enemy = [Actor {
        kind: ActorKind::Enemy,
        pos: at,
        yaw: 0.0,
    }];
    let flash = |muzzle| {
        let mut fx = CombatFx::new();
        fx.shot(&ShotFx {
            by_player: false,
            muzzle,
            end: muzzle,
            normal: v(-1.0, 0.0, 0.0),
            impact: Impact::None,
        });
        let draws = draws_of(&r, Some(&mut fx));
        changed(
            &drawn(&r, &enemy, &draws_of(&r, None)),
            &drawn(&r, &enemy, &draws),
        )
    };
    // Firing back along -X toward the eye at x = -4.
    let outside = flash(enemy_muzzle(at, v(-4.0, 0.0, 0.0)));
    let inside = flash(at);
    assert!(outside > 50, "{outside} pixels");
    assert!(
        outside > inside * 2,
        "{outside} against {inside} inside the hull"
    );
}

#[test]
fn different_materials_give_different_pictures() {
    let r = renderer(false);
    let picture = |material| {
        let mut fx = CombatFx::new();
        fx.shot(&ShotFx {
            by_player: true,
            muzzle: v(-3.5, -0.2, -0.2),
            end: v(1.0, 0.0, 0.0),
            normal: v(-1.0, 0.0, 0.0),
            impact: Impact::Surface {
                material,
                tint: 0xff90_3020,
            },
        });
        // Long enough to arrive and for the sparks and chips to spread.
        for _ in 0..12 {
            fx.advance(FRAME, r.scene().bsp());
        }
        assert_eq!(fx.bolts(), 0);
        frame(&r, Some(&mut fx))
    };
    let base = frame(&r, None);
    let (metal, brick) = (picture(METAL), picture(BRICK));
    assert!(changed(&base, &metal) > 20, "metal shows");
    assert!(changed(&base, &brick) > 20, "brick shows");
    assert!(changed(&metal, &brick) > 20, "and they differ");
}

#[test]
fn an_explosion_behind_a_wall_is_charged_less_than_the_same_one_in_the_open() {
    let r = renderer(true);
    let scene = frame(&r, None);
    let cam = r.camera(&view(), W, H).expect("a camera");
    let (w, h) = (W as f32, H as f32);
    let explosion = |at| {
        let mut fx = CombatFx::new();
        fx.drone_explosion(at, DRONE_TINT);
        for _ in 0..20 {
            fx.advance(FRAME, r.scene().bsp());
        }
        let mut draws = ParticleDraws::default();
        fx.collect(&cam, r.fog(), None, &mut draws);
        draws
    };
    // The same distance from the eye, so the same size on screen; one is on
    // the eye's side of the divider at x = -1 and one past it.
    let (front, behind) = (explosion(v(-2.0, 0.0, 0.0)), explosion(v(2.0, 0.0, 0.0)));
    assert!(!behind.is_empty());
    let open = cost_of(&front, Some(&scene), w, h);
    let walled = cost_of(&behind, Some(&scene), w, h);
    assert!(walled < open * 0.7, "behind {walled} against front {open}");
    assert!(
        cost_of(&behind, None, w, h) > walled,
        "and it is the depth that made the difference"
    );
}

fn session_frame(script: &str, frames: u64) -> Vec<u32> {
    let cfg = SessionConfig {
        quality: Quality::Medium,
        ..SessionConfig::default()
    };
    let materials = assets::load_materials(None).expect("textures");
    let mut s = GameSession::new(&levels::corridor_level(), materials, &cfg).expect("a level");
    let script = Script::parse(script).expect("a script");
    let dt = Duration::from_micros(16_667);
    let mut p = HeadlessPlatform::new(160, 90, dt, script).with_frame_limit(frames);
    let opts = RunOptions {
        base: (160, 90),
        adaptive: false,
        cap_fps: None,
        autoclose: None,
        shot_times: vec![],
        shot_stem: None,
        watch: None,
    };
    run(&mut p, &mut s, &opts);
    p.last_frame().expect("a frame").pixels.clone()
}

#[test]
fn a_scripted_shot_shows_and_is_pixel_identical_twice() {
    const FIRE: &str = "at 0.1s fire for 0.5s\n";
    // Two frames after the first shot, with its flash and bolt on screen.
    const IN_FLIGHT: u64 = 8;
    let shooting = session_frame(FIRE, IN_FLIGHT);
    assert_eq!(shooting, session_frame(FIRE, IN_FLIGHT), "effects replay");
    let idle = session_frame("", IN_FLIGHT);
    // Above the HUD, whose ammo count changes with a shot anyway.
    let world = 160 * 60;
    let same = shooting[..world].iter().zip(&idle[..world]);
    let differ = same.filter(|(a, b)| a != b).count();
    assert!(differ > 5, "the bolt draws: {differ} pixels");
}
