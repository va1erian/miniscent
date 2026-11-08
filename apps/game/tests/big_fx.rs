//! The blast and the shatter through a real level: what the view feels, and
//! that the budget holds at their peak.

use std::time::Duration;

use miniscent_game::app::{RunOptions, run};
use miniscent_game::{GameSession, SessionConfig, assets, levels};
use mscent_game::fx::budget::{Limits, cost_of};
use mscent_game::fx::combat::{CombatFx, ViewKick};
use mscent_math::{Aabb, Vec3, Vec3d};
use mscent_platform::{HeadlessPlatform, Script};
use mscent_render::{BuildConfig, LevelRenderer, Quality, View};
use mscent_world::materials::{BRICK, LIGHTPANEL};
use mscent_world::{Brush, Entity, Level, Op};

const FRAME: u64 = 16_666_667;
const W: u32 = 320;
const H: u32 = 180;

fn v(x: f32, y: f32, z: f32) -> Vec3 {
    Vec3::new(x, y, z)
}

/// A room, split by a sealed slab at x in `[-1, 1]`.
fn divided_room() -> LevelRenderer {
    let d = Vec3d::new;
    let cuboid = |a, b, op, order| Brush::cuboid(a, b, op, order).expect("a brush");
    let level = Level {
        kinds: Vec::new(),
        brushes: vec![
            cuboid(d(-20.0, -20.0, -20.0), d(20.0, 20.0, 20.0), Op::Subtract, 0),
            cuboid(d(-1.0, -21.0, -21.0), d(1.0, 21.0, 21.0), Op::Add, 1),
        ],
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
        luxel_size: 2.0,
        shadow_samples: 1,
        ..BuildConfig::default()
    };
    let materials = assets::load_materials(None).expect("textures");
    LevelRenderer::from_level(&level, materials, &cfg).expect("a level")
}

fn blast(r: &LevelRenderer) -> CombatFx {
    let mut fx = CombatFx::new();
    fx.blast(v(-10.0, 0.0, 0.0), 12.0, r.scene().bsp());
    fx
}

#[test]
fn a_blast_shakes_and_blinds_a_view_that_can_see_it_then_lets_go() {
    let r = divided_room();
    let mut fx = blast(&r);
    let eye = v(-12.0, 3.0, 0.0);
    let first = fx.view_kick(r.scene(), eye);
    assert!(first.white > 100, "{first:?}");
    let mut shaken = false;
    for _ in 0..3 {
        fx.advance(FRAME, r.scene().bsp());
        shaken |= fx.view_kick(r.scene(), eye).shake != Vec3::ZERO;
    }
    assert!(shaken);
    for _ in 0..40 {
        fx.advance(FRAME, r.scene().bsp());
    }
    assert_eq!(fx.view_kick(r.scene(), eye), ViewKick::default());
}

#[test]
fn a_blast_behind_a_wall_does_nothing_to_the_view() {
    let r = divided_room();
    let mut fx = blast(&r);
    fx.advance(FRAME, r.scene().bsp());
    let kick = fx.view_kick(r.scene(), v(10.0, 0.0, 0.0));
    assert_eq!(kick, ViewKick::default());
}

#[test]
fn a_distant_view_feels_less_and_a_shake_never_enters_a_wall() {
    let r = divided_room();
    let mut fx = blast(&r);
    let near = fx.view_kick(r.scene(), v(-11.0, 0.0, 0.0)).white;
    let far = fx.view_kick(r.scene(), v(-19.0, 15.0, 15.0)).white;
    assert!(far < near, "{far} against {near}");
    // Right against the room's wall, where a shake could cross it.
    for _ in 0..20 {
        fx.advance(FRAME, r.scene().bsp());
        let eye = v(-19.999, 0.0, 0.0);
        let kick = fx.view_kick(r.scene(), eye);
        assert!(!r.scene().bsp().is_solid((eye + kick.shake).to_f64()));
    }
}

#[test]
fn a_blast_stays_within_budget_at_every_frame_and_at_every_quality() {
    let r = divided_room();
    let view = View::from_angles(v(-18.0, 0.0, 0.0), -core::f32::consts::FRAC_PI_2, 0.0, 0.0);
    let cam = r.camera(&view, W, H).expect("a camera");
    let (w, h) = (W as f32, H as f32);
    for quality in [Quality::High, Quality::Medium, Quality::Low] {
        let mut fx = CombatFx::new();
        fx.set_quality(quality);
        fx.blast(v(-10.0, 0.0, 0.0), 12.0, r.scene().bsp());
        let mut draws = CombatFx::new_draws();
        let budget = Limits::of(quality).screens * w * h;
        let mut peak = 0.0f32;
        for _ in 0..120 {
            fx.advance(FRAME, r.scene().bsp());
            fx.collect(&cam, None, None, &mut draws);
            peak = peak.max(cost_of(&draws, None, w, h));
        }
        assert!(peak > budget * 0.5, "{quality:?} used {peak} of {budget}");
        // Essential particles are never cut, and the blast's flash is small.
        assert!(peak <= budget * 1.1, "{quality:?} used {peak} of {budget}");
    }
}

#[test]
fn hostile_blasts_and_shatters_do_nothing_and_do_not_panic() {
    let r = divided_room();
    let mut fx = CombatFx::new();
    for x in [f32::NAN, f32::INFINITY] {
        fx.blast(v(x, 0.0, 0.0), 12.0, r.scene().bsp());
        fx.blast(v(0.0, 0.0, 0.0), x, r.scene().bsp());
        let bad = Aabb {
            min: v(x, 0.0, 0.0),
            max: v(1.0, 1.0, 1.0),
        };
        fx.shatter(bad, BRICK, 0);
    }
    fx.blast(v(0.0, 0.0, 0.0), -1.0, r.scene().bsp());
    let live = |fx: &CombatFx| (fx.live(), fx.live_lights());
    fx.blast(v(0.0, 0.0, 0.0), 0.0, r.scene().bsp());
    let inverted = Aabb {
        min: v(1.0, 1.0, 1.0),
        max: v(0.0, 0.0, 0.0),
    };
    fx.shatter(inverted, BRICK, 0);
    assert_eq!(live(&fx), (0, 0));
    assert_eq!(
        fx.view_kick(r.scene(), v(f32::NAN, 0.0, 0.0)),
        ViewKick::default()
    );
}

fn shattered(size: f32, material: u16) -> CombatFx {
    let mut fx = CombatFx::new();
    let bounds = Aabb {
        min: v(-size / 2.0, -size / 2.0, -size / 2.0),
        max: v(size / 2.0, size / 2.0, size / 2.0),
    };
    fx.shatter(bounds, material, 0x0090_3020);
    fx
}

#[test]
fn a_bigger_brush_makes_more_debris_up_to_a_limit_and_each_material_differs() {
    let (small, medium) = (shattered(1.0, BRICK).live(), shattered(4.0, BRICK).live());
    assert!(medium > small, "{medium} against {small}");
    let huge = shattered(10.0, BRICK).live();
    let vast = shattered(1000.0, BRICK).live();
    assert_eq!(huge, vast, "clamped");
    assert!(vast <= 4096);
    assert_ne!(
        shattered(2.0, BRICK).live(),
        shattered(2.0, LIGHTPANEL).live()
    );
    assert_eq!(
        shattered(2.0, LIGHTPANEL).live_lights(),
        1,
        "its dying flash"
    );
}

#[test]
fn a_thousand_unit_brush_stays_within_budget() {
    let r = divided_room();
    let view = View::from_angles(v(-10.0, 0.0, 0.0), -core::f32::consts::FRAC_PI_2, 0.0, 0.0);
    let cam = r.camera(&view, W, H).expect("a camera");
    let (w, h) = (W as f32, H as f32);
    let mut fx = CombatFx::new();
    // Around the eye, as close as debris gets.
    let bounds = Aabb {
        min: v(-15.0, -5.0, -5.0),
        max: v(-5.0, 5.0, 5.0),
    };
    fx.set_quality(Quality::Low);
    fx.shatter(bounds, BRICK, 0x0090_3020);
    let mut draws = CombatFx::new_draws();
    let budget = Limits::of(Quality::Low).screens * w * h;
    let mut peak = 0.0f32;
    for _ in 0..30 {
        fx.advance(FRAME, r.scene().bsp());
        fx.collect(&cam, None, None, &mut draws);
        peak = peak.max(cost_of(&draws, None, w, h));
    }
    // Its counts are capped so low that it fits with nothing cut, even at Low.
    assert!(peak > 0.0 && peak <= budget, "{peak} of {budget}");
    assert_eq!(fx.keep(), Default::default());
}

/// The last of `frames` of a session, after a blast near the eye if `blast`.
fn session_frame(blast: bool, frames: u64) -> Vec<u32> {
    let cfg = SessionConfig {
        quality: Quality::Medium,
        ..SessionConfig::default()
    };
    let materials = assets::load_materials(None).expect("textures");
    let mut s = GameSession::new(&levels::corridor_level(), materials, &cfg).expect("a level");
    if blast {
        s.blast(s.game().player.pos(), 12.0);
    }
    let script = Script::parse("").expect("a script");
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
fn a_session_shows_a_blast_replays_it_exactly_and_returns_to_normal() {
    let mean = |px: &[u32]| px.iter().map(|&c| (c >> 8) & 0xff).sum::<u32>() / px.len() as u32;
    let blast = session_frame(true, 2);
    assert_eq!(blast, session_frame(true, 2), "a scripted run replays it");
    let (lit, plain) = (mean(&blast), mean(&session_frame(false, 2)));
    assert!(lit > plain + 40, "overlay and fire: {lit} against {plain}");
    // Past the longest life in the table, so nothing of it is left, and effects
    // never touch the game, so the frame is what it would have been.
    const LONG_AFTER: u64 = 300;
    assert!(
        session_frame(true, LONG_AFTER) == session_frame(false, LONG_AFTER),
        "the blast is gone and changed nothing"
    );
}

#[test]
fn a_blast_with_a_huge_radius_does_not_panic_and_fades_like_any_other() {
    let r = divided_room();
    for radius in [1.0e6, 1.0e30, f32::MAX] {
        let mut fx = CombatFx::new();
        fx.blast(v(-10.0, 0.0, 0.0), radius, r.scene().bsp());
        let cam_view = View::from_angles(v(-18.0, 0.0, 0.0), 0.0, 0.0, 0.0);
        let cam = r.camera(&cam_view, W, H).expect("a camera");
        let mut draws = CombatFx::new_draws();
        for _ in 0..40 {
            fx.advance(FRAME, r.scene().bsp());
            let kick = fx.view_kick(r.scene(), v(-12.0, 0.0, 0.0));
            assert!(kick.shake.is_finite() && kick.white <= 256);
            fx.collect(&cam, None, None, &mut draws);
            fx.lights(r.scene());
        }
    }
}

#[test]
fn a_pushed_light_stays_on_its_own_side_of_a_wall() {
    let r = divided_room();
    let mut fx = CombatFx::new();
    // Close enough that a push of 7 units ends in open space past the slab.
    fx.blast(v(-4.0, 0.0, 0.0), 12.0, r.scene().bsp());
    let lights = fx.lights(r.scene());
    assert_eq!(lights.len(), 4);
    for light in lights {
        assert!(light.pos().x < -1.0, "{:?}", light.pos());
    }
}

#[test]
fn clear_forgets_the_blast_lights_and_the_cut() {
    let r = divided_room();
    let view = View::from_angles(v(-18.0, 0.0, 0.0), -core::f32::consts::FRAC_PI_2, 0.0, 0.0);
    let cam = r.camera(&view, W, H).expect("a camera");
    let mut fx = CombatFx::new();
    fx.set_quality(Quality::Low);
    fx.blast(v(-10.0, 0.0, 0.0), 12.0, r.scene().bsp());
    let mut draws = CombatFx::new_draws();
    for _ in 0..12 {
        fx.advance(FRAME, r.scene().bsp());
        fx.collect(&cam, None, None, &mut draws);
    }
    assert_ne!(
        fx.keep(),
        Default::default(),
        "the budget had cut something"
    );
    fx.clear();
    assert_eq!(
        (fx.live(), fx.live_lights(), fx.bolts(), fx.decals()),
        (0, 0, 0, 0)
    );
    assert_eq!(fx.keep(), Default::default());
    assert_eq!(
        fx.view_kick(r.scene(), v(-12.0, 0.0, 0.0)),
        ViewKick::default()
    );
    fx.collect(&cam, None, None, &mut draws);
    assert!(draws.is_empty());
}

#[test]
fn the_pause_menu_does_not_hold_the_shake() {
    let cfg = SessionConfig::default();
    let materials = assets::load_materials(None).expect("textures");
    let mut s = GameSession::new(&levels::corridor_level(), materials, &cfg).expect("a level");
    s.blast(s.game().player.pos(), 12.0);
    let dt = Duration::from_micros(16_667);
    s.step(dt, &[]);
    s.render(160, 90);
    let shaken = s.view().pos;
    s.step(dt, &[mscent_platform::Event::FocusLost]);
    s.render(160, 90);
    let (still, plain) = (s.view().pos, s.game().player.pos());
    assert_ne!(shaken, still, "it was shaking, and it stops");
    assert!((still - plain).length() < 0.2, "only the hover bob is left");
}
