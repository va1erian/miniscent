//! The showcase enemies can be shot where they are drawn, and an enemy that
//! hurts the player is close enough to explain it.

use mscent_game::{Arena, Event, Game, Input};
use mscent_math::Vec3;
use mscent_render::View;
use mscent_world::showcase_level;

fn aim_at(game: &mut Game, target: Vec3) {
    let d = target - game.player.pos();
    let yaw = (-d.x).atan2(d.y);
    let pitch = d.z.atan2((d.x * d.x + d.y * d.y).sqrt());
    game.player.orientation = View::from_angles(Vec3::ZERO, yaw, pitch, 0.0).orientation;
}

#[test]
fn aiming_at_the_start_room_enemy_hits_it() {
    let mut game = Game::new(Arena::from_level(&showcase_level()).expect("arena"));
    let enemy = game.enemies[1].pos();
    aim_at(&mut game, enemy);
    let fwd = game.player.forward();
    let want = (enemy - game.player.pos()).normalized().expect("dir");
    assert!(fwd.dot(want) > 0.999, "aim {fwd:?} vs {want:?}");
    let fire = Input {
        fire: true,
        ..Input::default()
    };
    game.step(&fire);
    let ev = game.take_events();
    assert!(
        ev.iter().any(|e| matches!(e, Event::EnemyHit(1))),
        "events {ev:?}, enemy at {enemy:?}"
    );
}

#[test]
fn an_enemy_that_hurts_the_player_announces_its_shot() {
    let mut game = Game::new(Arena::from_level(&showcase_level()).expect("arena"));
    let idle = Input::default();
    let mut fired = false;
    let mut hurt = false;
    for _ in 0..1200 {
        game.step(&idle);
        for e in game.take_events() {
            fired |= matches!(e, Event::EnemyFired(_));
            if matches!(e, Event::PlayerHurt(_)) {
                assert!(fired, "damage arrived with no visible shot before it");
                hurt = true;
            }
        }
    }
    assert!(hurt, "the start-room enemy never attacked");
}

#[test]
fn r_is_thrust_up_in_play_and_never_restarts_mid_run() {
    use miniscent_game::{GameSession, SessionConfig, assets};
    use mscent_platform::{Event as Ev, Key};
    use std::time::Duration;
    let materials = assets::load_materials(None).expect("textures");
    let mut s =
        GameSession::new(&showcase_level(), materials, &SessionConfig::default()).expect("level");
    let key = |pressed| Ev::Key {
        code: Key::Char('R'),
        pressed,
    };
    s.step(Duration::from_millis(500), &[]);
    let before = s.game().ticks;
    s.step(Duration::from_millis(100), &[key(true)]);
    s.step(Duration::from_millis(500), &[key(true)]);
    s.step(Duration::from_millis(10), &[key(false)]);
    assert!(s.game().ticks > before, "the run was reset by R");
    assert!(s.game().player.pos().z > 2.0, "R did not thrust up");
}
