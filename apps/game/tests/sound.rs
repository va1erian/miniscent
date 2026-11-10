//! The game's sound effects, from real events in a real level.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use miniscent_game::app::{RunOptions, run};
use miniscent_game::sound::{Cue, Sounds};
use miniscent_game::{GameSession, SessionConfig, assets, levels};
use mscent_audio::Play;
use mscent_game::{Arena, Event, Game, Phase};
use mscent_math::Vec3;
use mscent_platform::{HeadlessPlatform, Script};
use mscent_render::{Quality, View};

type Log = Arc<Mutex<Vec<Play>>>;

fn recorder() -> (Sounds, Log) {
    let (bank, skipped) = assets::load_sounds(None).expect("sounds");
    assert!(skipped.is_empty(), "{skipped:?}");
    let log = Log::default();
    let sink = log.clone();
    (
        Sounds::new(bank, move |p| sink.lock().unwrap().push(p)),
        log,
    )
}

fn session(with_sound: Option<Sounds>) -> GameSession {
    let cfg = SessionConfig {
        quality: Quality::Medium,
        ..SessionConfig::default()
    };
    let materials = assets::load_materials(None).expect("textures");
    let mut s = GameSession::new(&levels::corridor_level(), materials, &cfg).expect("level");
    if let Some(sounds) = with_sound {
        s.set_sounds(sounds);
    }
    s
}

const WIN: &str = include_str!("../../../assets/scripts/corridor_win.txt");

fn play_win(s: &mut GameSession) {
    let script = Script::parse(WIN).expect("script");
    let mut p = HeadlessPlatform::new(160, 90, Duration::from_micros(16_667), script)
        .with_frame_limit(1200);
    let opts = RunOptions {
        base: (160, 90),
        adaptive: false,
        cap_fps: None,
        autoclose: None,
        shot_times: vec![],
        shot_stem: None,
        watch: None,
    };
    run(&mut p, s, &opts);
}

fn count(log: &Log, cue: Cue) -> usize {
    let (bank, _) = assets::load_sounds(None).unwrap();
    let want = bank.get(cue.file()).unwrap();
    log.lock()
        .unwrap()
        .iter()
        .filter(|p| p.sound == *want)
        .count()
}

#[test]
fn the_win_run_makes_the_sounds_of_a_fight_and_a_win() {
    let (sounds, log) = recorder();
    let mut s = session(Some(sounds));
    play_win(&mut s);
    assert_eq!(s.phase(), Phase::Won);
    assert!(count(&log, Cue::PlayerShot) >= 1, "the player fired");
    assert_eq!(count(&log, Cue::EnemyDeath), 1, "one enemy died once");
    assert_eq!(count(&log, Cue::Win), 1, "the goal sounded once");
    assert_eq!(count(&log, Cue::PlayerDeath), 0);
    for p in log.lock().unwrap().iter() {
        assert!((0.0..=1.0).contains(&p.gain) && (-1.0..=1.0).contains(&p.pan));
    }
}

#[test]
fn sound_does_not_change_the_game() {
    let mut quiet = session(None);
    let (sounds, _) = recorder();
    let mut loud = session(Some(sounds));
    play_win(&mut quiet);
    play_win(&mut loud);
    assert_eq!(quiet.phase(), loud.phase());
    assert_eq!(quiet.game().ticks, loud.game().ticks);
    assert_eq!(quiet.game().player.pos(), loud.game().player.pos());
}

fn corridor_game() -> Game {
    Game::new(Arena::from_level(&levels::corridor_level()).unwrap())
}

#[test]
fn an_enemy_off_to_one_side_pans_that_way_and_a_far_one_is_quieter() {
    let (mut sounds, log) = recorder();
    let mut game = corridor_game();
    // Face along the corridor the way a session does (its spawn yaw is 0).
    let face = |game: &mut Game, yaw: f32| {
        game.player.orientation = View::from_angles(Vec3::ZERO, yaw, 0.0, 0.0).orientation;
    };
    face(&mut game, 0.0);
    // The enemy is 49 units dead ahead: centred, and quiet with distance.
    sounds.on_event(&game, &Event::EnemyFired(0));
    {
        let log = log.lock().unwrap();
        assert!(log[0].pan.abs() < 1e-3, "{}", log[0].pan);
        assert!(log[0].gain < 0.3 && log[0].gain > 0.1, "{}", log[0].gain);
    }
    face(&mut game, 0.7);
    sounds.on_event(&game, &Event::EnemyFired(0));
    face(&mut game, -0.7);
    sounds.on_event(&game, &Event::EnemyFired(0));
    let log = log.lock().unwrap();
    let pans: Vec<f32> = log.iter().map(|p| p.pan).collect();
    assert!(
        pans[1] * pans[2] < 0.0,
        "turning the other way flips the side: {pans:?}"
    );
    assert!(pans[1].abs() > 0.3 && pans[2].abs() > 0.3);
}

#[test]
fn a_killing_blow_sounds_the_death_not_a_second_pain() {
    let (mut sounds, log) = recorder();
    let mut game = corridor_game();
    game.enemies[0].damage(10_000);
    sounds.on_event(&game, &Event::EnemyHit(0));
    assert!(
        log.lock().unwrap().is_empty(),
        "a dead enemy makes no pain sound"
    );
    sounds.on_event(&game, &Event::EnemyKilled(0));
    assert_eq!(log.lock().unwrap().len(), 1);
}

#[test]
fn events_that_name_nothing_that_exists_are_silent_not_a_panic() {
    let (mut sounds, log) = recorder();
    let game = corridor_game();
    for e in [
        Event::EnemyFired(9_999),
        Event::PropUsed(9_999),
        Event::PropHit(9_999),
        Event::ZoneEntered(3),
    ] {
        sounds.on_event(&game, &e);
    }
    assert!(log.lock().unwrap().iter().all(|p| p.gain.is_finite()));
}
