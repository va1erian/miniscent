//! Regression tests for input, determinism and cleanup bugs found in review.

use std::time::Duration;

use miniscent_game::app::{RunOptions, run};
use miniscent_game::{GameSession, SessionConfig, assets, levels};
use mscent_platform::{Event, HeadlessPlatform, Key, MouseButton, Script};
use mscent_render::Quality;

const DT: Duration = Duration::from_micros(16_667);

fn session() -> GameSession {
    let cfg = SessionConfig {
        quality: Quality::Low,
        ..SessionConfig::default()
    };
    let materials = assets::load_materials(None).expect("textures");
    GameSession::new(&levels::corridor_level(), materials, &cfg).expect("level")
}

fn opts() -> RunOptions {
    RunOptions {
        base: (96, 54),
        adaptive: false,
        cap_fps: None,
        autoclose: None,
        shot_times: vec![],
        shot_stem: None,
        watch: None,
    }
}

#[test]
fn mouse_is_released_when_the_run_ends_while_playing() {
    let mut s = session();
    let mut p = HeadlessPlatform::new(96, 54, DT, Script::empty());
    let o = RunOptions {
        autoclose: Some(Duration::from_millis(200)),
        ..opts()
    };
    run(&mut p, &mut s, &o);
    assert!(!p.mouse_captured(), "the cursor grab leaked past the run");
}

#[test]
fn mouse_is_released_when_the_platform_quits_while_playing() {
    let mut s = session();
    let mut p = HeadlessPlatform::new(96, 54, DT, Script::empty()).with_frame_limit(5);
    run(&mut p, &mut s, &opts());
    assert!(!p.mouse_captured(), "the cursor grab leaked past the run");
}

#[test]
fn headless_runs_are_pixel_deterministic() {
    let frame = || {
        let mut s = session();
        let script = Script::parse("at 0s hold W\nat 0s mouse 100 0 over 1s\n").unwrap();
        let mut p = HeadlessPlatform::new(320, 180, DT, script).with_frame_limit(40);
        run(
            &mut p,
            &mut s,
            &RunOptions {
                base: (320, 180),
                ..opts()
            },
        );
        p.last_frame().cloned().unwrap()
    };
    let (a, b) = (frame(), frame());
    // Wall-clock timings must not leak into the picture; slow the second run.
    assert_eq!(a, b, "two identical scripted runs drew different pixels");
}

#[test]
fn clicking_to_resume_does_not_also_fire() {
    let mut s = session();
    let key = |pressed| Event::Key {
        code: Key::Escape,
        pressed,
    };
    s.step(Duration::from_millis(100), &[]);
    s.step(Duration::from_millis(10), &[key(true), key(false)]);
    assert!(!s.step(Duration::from_millis(10), &[]).capture_mouse);
    let click = Event::MouseButton {
        button: MouseButton::Left,
        pressed: true,
    };
    let out = s.step(Duration::from_millis(100), &[click]);
    assert!(out.capture_mouse, "the click resumed");
    assert!(s.game().weapon.ready(), "the resume click fired the weapon");
}
