//! Headless end-to-end runs: scripted input through the platform trait into
//! a real baked level, checked on pixels and game state.

use std::time::Duration;

use miniscent_game::app::{RunOptions, run};
use miniscent_game::{GameSession, SessionConfig, assets, levels};
use mscent_game::{Phase, Screen};
use mscent_platform::{Event, HeadlessPlatform, Key, Script};
use mscent_render::Quality;

const DT: Duration = Duration::from_micros(16_667);

fn session() -> GameSession {
    let cfg = SessionConfig {
        quality: Quality::Medium,
        ..SessionConfig::default()
    };
    let materials = assets::load_materials(None).expect("textures");
    GameSession::new(&levels::corridor_level(), materials, &cfg).expect("level")
}

fn opts() -> RunOptions {
    RunOptions {
        base: (160, 90),
        adaptive: false,
        cap_fps: None,
        autoclose: None,
        shot_times: vec![],
        shot_stem: None,
    }
}

fn play(script: &str, frames: u64, s: &mut GameSession) -> HeadlessPlatform {
    let script = Script::parse(script).expect("script");
    let mut p = HeadlessPlatform::new(160, 90, DT, script).with_frame_limit(frames);
    run(&mut p, s, &opts());
    p
}

const WIN: &str = include_str!("../../../assets/scripts/corridor_win.txt");

#[test]
fn scripted_run_kills_the_enemy_reaches_the_goal_and_shows_the_win_screen() {
    let mut s = session();
    let p = play(WIN, 1200, &mut s);
    assert_eq!(s.phase(), Phase::Won);
    assert_eq!(s.game().enemies_alive(), 0, "the enemy was shot");
    assert_eq!(s.screen(), Screen::Won);
    assert!(
        !p.mouse_captured(),
        "the mouse is released on the win screen"
    );
    // The win screen is a dimmed frame: it must differ from a live frame.
    let win = p.last_frame().expect("frame");
    let live = {
        let mut s2 = session();
        play("", 2, &mut s2).last_frame().cloned().expect("frame")
    };
    assert_ne!(win.pixels, live.pixels);
    let bright = |f: &mscent_platform::Frame| f.pixels.iter().filter(|&&c| c & 0xff > 0x60).count();
    assert!(bright(win) > 50, "text pixels on the win screen");
}

#[test]
fn frames_are_not_flat_and_are_deterministic() {
    let mut a = session();
    let mut b = session();
    let pa = play("at 0.1s hold W\n", 20, &mut a);
    let pb = play("at 0.1s hold W\n", 20, &mut b);
    let fa = pa.last_frame().unwrap();
    // The fps/ms readout (top 20 rows) is wall-clock; the rest is deterministic.
    let below = 20 * 160;
    assert_eq!(fa.pixels[below..], pb.last_frame().unwrap().pixels[below..]);
    let mut colours = fa.pixels.clone();
    colours.sort_unstable();
    colours.dedup();
    assert!(colours.len() > 100, "{} distinct colours", colours.len());
}

#[test]
fn mouse_look_turns_the_view_and_roll_tilts_it() {
    let mut s = session();
    let before = s.view().forward();
    s.step(DT, &[Event::MouseMotion { dx: 100.0, dy: 0.0 }]);
    let after = s.view().forward();
    assert!(
        after.x > before.x + 0.1,
        "moving the mouse right turns right: {after:?}"
    );
    let up_before = s
        .view()
        .orientation
        .rotate(mscent_math::Vec3::new(0.0, 1.0, 0.0));
    let q = Event::Key {
        code: Key::Char('Q'),
        pressed: true,
    };
    s.step(DT, &[q]);
    for _ in 0..60 {
        s.step(DT, &[]);
    }
    let up_after = s
        .view()
        .orientation
        .rotate(mscent_math::Vec3::new(0.0, 1.0, 0.0));
    assert!(up_before.dot(up_after) < 0.9, "roll moved the up vector");
}

#[test]
fn escape_pauses_then_quits_and_r_restarts() {
    let mut s = session();
    let esc = Event::Key {
        code: Key::Escape,
        pressed: true,
    };
    let out = s.step(
        DT,
        &[Event::Key {
            code: Key::Char('W'),
            pressed: true,
        }],
    );
    assert!(out.capture_mouse && !out.quit);
    for _ in 0..30 {
        s.step(DT, &[]);
    }
    let moved = s.game().player.pos();
    assert!(moved.y > 3.0);
    let out = s.step(DT, &[esc]);
    assert!(
        !out.capture_mouse && !out.quit,
        "first Esc releases the mouse"
    );
    assert_eq!(s.screen(), Screen::Paused);
    let frozen = s.game().player.pos();
    for _ in 0..30 {
        s.step(DT, &[]);
    }
    assert_eq!(s.game().player.pos(), frozen, "paused game does not move");
    s.step(
        DT,
        &[Event::Key {
            code: Key::Escape,
            pressed: false,
        }],
    );
    let out = s.step(DT, &[esc]);
    assert!(out.quit, "second Esc quits");
    s.step(
        DT,
        &[Event::Key {
            code: Key::Char('R'),
            pressed: true,
        }],
    );
    assert_eq!(s.screen(), Screen::Hidden);
    assert_eq!(s.game().player.pos().y, 3.0, "restarted at the spawn");
}

#[test]
fn bad_levels_report_errors_instead_of_panicking() {
    let level = mscent_world::Level::default();
    let materials = assets::load_materials(None).expect("textures");
    assert!(GameSession::new(&level, materials, &SessionConfig::default()).is_err());
}
