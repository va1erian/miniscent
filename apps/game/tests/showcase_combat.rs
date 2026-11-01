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
