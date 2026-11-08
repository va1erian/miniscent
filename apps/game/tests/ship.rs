//! The player's ship is cosmetic: a session that has one plays the same game and
//! draws the same software frames as one that has none, whatever it does on a GPU.
#![cfg(feature = "hardware")]

use std::time::Duration;

use miniscent_game::{GameSession, SessionConfig, assets, levels};
use mscent_game::Arena;
use mscent_platform::{HeadlessPlatform, Platform, Script};
use mscent_render::{BuildConfig, LevelRenderer, Quality};

const SIZE: (u32, u32) = (160, 90);
const DT: Duration = Duration::from_micros(16_667);
const PLAY: &str = "at 0.1s hold W for 1s\nat 0.3s fire for 0.5s\nat 0.5s hold D for 0.5s\n";

fn config() -> SessionConfig {
    SessionConfig {
        quality: Quality::Medium,
        ..SessionConfig::default()
    }
}

/// A session made as the game makes it, with a ship.
fn with_ship() -> GameSession {
    let materials = assets::load_materials(None).expect("textures");
    GameSession::new(&levels::corridor_level(), materials, &config()).expect("a level")
}

/// The same level and game, made the way a test does, with no ship.
fn without_ship() -> GameSession {
    let level = levels::corridor_level();
    let materials = assets::load_materials(None).expect("textures");
    let arena = Arena::from_level(&level).expect("an arena");
    let renderer =
        LevelRenderer::from_level(&level, materials, &BuildConfig::default()).expect("a level");
    GameSession::with_renderer(arena, renderer, &config())
}

#[test]
fn a_session_with_a_ship_plays_and_draws_in_software_as_one_without() {
    let (mut a, mut b) = (with_ship(), without_ship());
    let script = Script::parse(PLAY).expect("a script");
    let mut platform = HeadlessPlatform::new(SIZE.0, SIZE.1, DT, script).with_frame_limit(90);
    for frame in 0..90 {
        let events = platform.poll_events();
        a.step(DT, &events);
        b.step(DT, &events);
        let pa = a.render(SIZE.0, SIZE.1).expect("a frame").pixels().to_vec();
        let pb = b.render(SIZE.0, SIZE.1).expect("a frame").pixels().to_vec();
        assert_eq!(pa, pb, "frame {frame}");
        let (ga, gb) = (a.game().player.pos(), b.game().player.pos());
        assert_eq!((ga.x, ga.y, ga.z), (gb.x, gb.y, gb.z), "frame {frame}");
        assert_eq!(a.game().player.health, b.game().player.health);
    }
    assert!(a.game().player.pos().length() > 0.0);
}

#[test]
fn a_chase_view_moves_only_the_camera() {
    let (mut a, mut b) = (with_ship(), with_ship());
    b.set_chase(true);
    let script = Script::parse(PLAY).expect("a script");
    let mut platform = HeadlessPlatform::new(SIZE.0, SIZE.1, DT, script).with_frame_limit(60);
    for _ in 0..60 {
        let events = platform.poll_events();
        a.step(DT, &events);
        b.step(DT, &events);
    }
    let (pa, pb) = (a.game().player.pos(), b.game().player.pos());
    assert_eq!((pa.x, pa.y, pa.z), (pb.x, pb.y, pb.z));
    let (va, vb) = (a.view().pos, b.view().pos);
    let behind = (va - vb).length();
    assert!(
        (behind - 3.0).abs() < 1.0,
        "the chase camera is {behind} from the first-person one"
    );
}

/// The chase view draws the ship, on the hardware renderer. Software never does:
/// its frame from behind the player is the frame a session with no ship draws.
#[test]
fn software_draws_no_ship_in_a_chase_view() {
    let (mut a, mut b) = (with_ship(), without_ship());
    a.set_chase(true);
    b.set_chase(true);
    let script = Script::parse(PLAY).expect("a script");
    let mut platform = HeadlessPlatform::new(SIZE.0, SIZE.1, DT, script).with_frame_limit(60);
    for frame in 0..60 {
        let events = platform.poll_events();
        a.step(DT, &events);
        b.step(DT, &events);
        let pa = a.render(SIZE.0, SIZE.1).expect("a frame").pixels().to_vec();
        let pb = b.render(SIZE.0, SIZE.1).expect("a frame").pixels().to_vec();
        assert_eq!(pa, pb, "frame {frame}");
    }
}
