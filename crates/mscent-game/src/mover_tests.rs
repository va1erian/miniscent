use crate::mover::{CLOSED, MAX_POSES, MoverError, WhenBlocked};
use crate::test_util::{FACE_PLUS_X, enemy, far_goal, open_level, start, v};
use crate::{Arena, Event, Game, Input, Phase, hitscan, line_of_sight};
use mscent_collide::{Pose, Solids};
use mscent_math::Vec3d;
use mscent_world::{Brush, Entity, Level, MoverGroup, Op};

fn vd(x: f64, y: f64, z: f64) -> Vec3d {
    Vec3d::new(x, y, z)
}

/// `open_level` plus one mover: a slab across x = 5 to 6, y and z -3 to 3.
fn door_level(entities: Vec<Entity>) -> Level {
    let mut level = open_level(entities);
    let door = Brush::cuboid(vd(5.0, -3.0, -3.0), vd(6.0, 3.0, 3.0), Op::Add, 99).unwrap();
    level.brushes.push(door);
    level.mover_groups.push(MoverGroup {
        name: "door".to_string(),
        tags: Vec::new(),
        brushes: vec![level.brushes.len() as u32 - 1],
    });
    level
}

fn door_game(player_at: mscent_math::Vec3) -> Game {
    let level = door_level(vec![start(player_at, FACE_PLUS_X), far_goal()]);
    Game::new(Arena::from_level(&level).unwrap())
}

fn open_pose() -> Pose {
    Pose::translated(vd(0.0, 0.0, 20.0))
}

fn run(game: &mut Game, steps: usize, input: &Input) {
    for _ in 0..steps {
        game.step(input);
    }
}

fn forward() -> Input {
    Input {
        thrust: v(0.0, 0.0, 1.0),
        ..Input::default()
    }
}

#[test]
fn a_closed_door_stops_the_player_and_an_open_one_does_not() {
    let mut game = door_game(v(0.0, 0.0, 0.0));
    run(&mut game, 480, &forward());
    assert!(
        game.player.pos().x < 5.0,
        "through a shut door: {:?}",
        game.player.pos()
    );
    let door = game.mover_index("door").unwrap();
    game.define_pose(door, "open", open_pose()).unwrap();
    game.move_mover(door, "open", 0.5).unwrap();
    run(&mut game, 480, &forward());
    assert!(
        game.player.pos().x > 6.0,
        "stuck at an open door: {:?}",
        game.player.pos()
    );
}

#[test]
fn a_door_arrives_eases_and_reports_once() {
    let mut game = door_game(v(0.0, 10.0, 0.0));
    let door = game.mover_index("door").unwrap();
    game.define_pose(door, "open", open_pose()).unwrap();
    assert_eq!(game.mover(door).unwrap().pose_name(), Some(CLOSED));
    game.move_mover(door, "open", 1.0).unwrap();
    game.take_events();
    run(&mut game, 12, &Input::default());
    let early = game.mover(door).unwrap().pose().offset.z;
    assert!(game.mover(door).unwrap().is_moving());
    assert!(
        early > 0.0 && early < 20.0 * 0.1,
        "a tenth of the time is less than a tenth of the way: {early}"
    );
    run(&mut game, 108, &Input::default());
    let mover = game.mover(door).unwrap();
    assert!(!mover.is_moving());
    assert_eq!(mover.pose_name(), Some("open"));
    assert_eq!(mover.pose().offset.z, 20.0);
    let events = game.take_events();
    let arrived = events.iter().filter(|e| **e == Event::MoverArrived(door));
    assert_eq!(arrived.count(), 1);
}

#[test]
fn a_door_stops_shots_and_sight_until_it_opens() {
    let mut game = door_game(v(0.0, 0.0, 0.0));
    let door = game.mover_index("door").unwrap();
    let (a, b) = (v(0.0, 0.0, 0.0), v(10.0, 0.0, 0.0));
    let solids = Solids::new(&game.arena.bsp, &game.solids);
    assert!(!line_of_sight(solids, a, b));
    let shot = hitscan(solids, a, v(1.0, 0.0, 0.0), 100.0, &[]);
    assert_eq!(shot.mover, Some(door));
    assert!((shot.distance - 5.0).abs() < 0.01, "{shot:?}");
    game.define_pose(door, "open", open_pose()).unwrap();
    game.move_mover(door, "open", 0.0).unwrap();
    run(&mut game, 1, &Input::default());
    let solids = Solids::new(&game.arena.bsp, &game.solids);
    assert!(line_of_sight(solids, a, b));
    assert_eq!(hitscan(solids, a, v(1.0, 0.0, 0.0), 8.0, &[]).mover, None);
}

#[test]
fn firing_at_a_door_reports_the_hit() {
    let mut game = door_game(v(0.0, 0.0, 0.0));
    let door = game.mover_index("door").unwrap();
    let fire = Input {
        fire: true,
        ..Input::default()
    };
    game.step(&fire);
    assert!(game.take_events().contains(&Event::MoverHit(door)));
}

#[test]
fn a_closed_door_hides_the_player_from_an_enemy() {
    let level = door_level(vec![
        start(v(0.0, 0.0, 0.0), FACE_PLUS_X),
        enemy(v(9.0, 0.0, 0.0)),
        far_goal(),
    ]);
    let mut game = Game::new(Arena::from_level(&level).unwrap());
    run(&mut game, 240, &Input::default());
    assert!(game.enemies[0].last_seen.is_none(), "it saw through a door");
    assert!(game.player.health > 0);
    let door = game.mover_index("door").unwrap();
    game.define_pose(door, "open", open_pose()).unwrap();
    game.move_mover(door, "open", 0.0).unwrap();
    run(&mut game, 60, &Input::default());
    assert!(
        game.enemies[0].last_seen.is_some(),
        "it should see once open"
    );
}

#[test]
fn a_door_pushes_the_player_and_enemies_ahead_of_it() {
    let level = door_level(vec![
        start(v(4.0, 0.0, 0.0), FACE_PLUS_X),
        enemy(v(4.0, 2.0, 0.0)),
        far_goal(),
    ]);
    let mut game = Game::new(Arena::from_level(&level).unwrap());
    let door = game.mover_index("door").unwrap();
    game.define_pose(door, "in", Pose::translated(vd(-6.0, 0.0, 0.0)))
        .unwrap();
    game.move_mover(door, "in", 0.5).unwrap();
    run(&mut game, 60, &Input::default());
    assert_eq!(game.mover(door).unwrap().pose_name(), Some("in"));
    assert!(game.player.pos().x < -1.4, "{:?}", game.player.pos());
    assert!(
        game.enemies[0].pos().x < -1.0,
        "{:?}",
        game.enemies[0].pos()
    );
}

/// A player pinned between the door and the room's far wall at x = -8 from
/// `open_level`; the door wants to reach the wall itself.
fn squeeze(when: WhenBlocked) -> (Game, usize) {
    let bounds = open_level(Vec::new()).brushes[0].bounds();
    let mut game = door_game(v(4.0, 0.0, 0.0));
    let door = game.mover_index("door").unwrap();
    let reach = f64::from(bounds.min.x) - 5.0;
    game.define_pose(door, "in", Pose::translated(vd(reach, 0.0, 0.0)))
        .unwrap();
    game.set_when_blocked(door, when).unwrap();
    game.move_mover(door, "in", 0.5).unwrap();
    (game, door)
}

#[test]
fn hold_waits_where_it_stopped_and_reports_once() {
    let (mut game, door) = squeeze(WhenBlocked::Hold);
    game.take_events();
    run(&mut game, 240, &Input::default());
    let mover = game.mover(door).unwrap();
    assert!(mover.is_moving(), "it is still trying");
    let held = mover.pose().offset.x;
    run(&mut game, 60, &Input::default());
    assert_eq!(game.mover(door).unwrap().pose().offset.x, held);
    let events = game.take_events();
    let blocked = events.iter().filter(|e| **e == Event::MoverBlocked(door));
    assert_eq!(blocked.count(), 1, "{events:?}");
    assert_eq!(game.player.health, crate::player::MAX_HEALTH);
}

#[test]
fn reverse_goes_back_to_where_it_came_from() {
    let (mut game, door) = squeeze(WhenBlocked::Reverse);
    run(&mut game, 360, &Input::default());
    let mover = game.mover(door).unwrap();
    assert!(!mover.is_moving());
    assert_eq!(mover.pose_name(), Some(CLOSED), "{:?}", mover.pose());
    assert_eq!(mover.pose(), &Pose::IDENTITY);
}

#[test]
fn crush_hurts_the_player_until_the_level_is_lost() {
    let (mut game, door) = squeeze(WhenBlocked::Crush {
        damage_per_second: 1000.0,
    });
    run(&mut game, 240, &Input::default());
    assert_eq!(game.phase, Phase::Lost, "health {}", game.player.health);
    assert!(game.take_events().contains(&Event::PlayerDied));
    assert!(game.mover(door).unwrap().is_moving());
}

#[test]
fn crush_kills_an_enemy_and_the_door_finishes() {
    let level = door_level(vec![
        start(v(-6.0, 6.0, 0.0), FACE_PLUS_X),
        enemy(v(4.0, 0.0, 0.0)),
        far_goal(),
    ]);
    let min_x = open_level(Vec::new()).brushes[0].bounds().min.x;
    let mut game = Game::new(Arena::from_level(&level).unwrap());
    let door = game.mover_index("door").unwrap();
    let pose = Pose::translated(vd(f64::from(min_x) - 5.0, 0.0, 0.0));
    game.define_pose(door, "in", pose).unwrap();
    let crush = WhenBlocked::Crush {
        damage_per_second: 2000.0,
    };
    game.set_when_blocked(door, crush).unwrap();
    game.enemies[0].state = crate::AiState::Patrol;
    game.move_mover(door, "in", 0.25).unwrap();
    let mut killed = false;
    for _ in 0..240 {
        game.step(&Input::default());
        killed |= game.take_events().contains(&Event::EnemyKilled(0));
    }
    assert!(killed, "{:?}", game.enemies[0].pos());
    assert_eq!(game.mover(door).unwrap().pose_name(), Some("in"));
}

#[test]
fn a_hinged_door_swings_on_its_hinge_and_shoves_what_it_meets() {
    let mut game = door_game(v(3.0, 2.0, 0.0));
    let door = game.mover_index("door").unwrap();
    // Hinged on its near edge at (5, -3), turning its free end toward -x.
    let swing = Pose::turned(
        v(0.0, 0.0, 1.0),
        core::f32::consts::FRAC_PI_2,
        vd(5.0, -3.0, 0.0),
    );
    game.define_pose(door, "open", swing).unwrap();
    game.move_mover(door, "open", 1.0).unwrap();
    run(&mut game, 120, &Input::default());
    assert_eq!(game.mover(door).unwrap().pose_name(), Some("open"));
    // It now lies along y = -3 from x = -1 to 5, and the player was moved aside.
    let p = game.player.pos();
    assert!(p.x < -1.4 || p.y < -3.4, "{p:?}");
}

#[test]
fn restart_puts_movers_back() {
    let mut game = door_game(v(0.0, 10.0, 0.0));
    let door = game.mover_index("door").unwrap();
    game.define_pose(door, "open", open_pose()).unwrap();
    game.move_mover(door, "open", 0.25).unwrap();
    run(&mut game, 60, &Input::default());
    game.restart();
    let mover = game.mover(door).unwrap();
    assert_eq!(mover.pose(), &Pose::IDENTITY);
    assert!(!mover.is_moving());
    let at_door = Solids::new(&game.arena.bsp, &game.solids);
    assert!(!line_of_sight(at_door, v(0.0, 0.0, 0.0), v(10.0, 0.0, 0.0)));
}

#[test]
fn the_same_inputs_replay_bit_exactly_with_movers() {
    let play = || {
        let mut game = door_game(v(4.0, 1.0, 0.5));
        let door = game.mover_index("door").unwrap();
        let swing = Pose::turned(v(0.3, 0.2, 1.0), 2.5, vd(5.0, -3.0, 0.0));
        game.define_pose(door, "a", swing).unwrap();
        game.define_pose(door, "b", Pose::translated(vd(-6.0, 1.0, 2.0)))
            .unwrap();
        let mut trail = Vec::new();
        for i in 0..1200 {
            if i % 200 == 0 {
                let pose = if i % 400 == 0 { "a" } else { "b" };
                game.move_mover(door, pose, 0.7).unwrap();
            }
            game.step(&forward());
            trail.push((game.player.pos(), *game.mover(door).unwrap().pose()));
        }
        trail
    };
    let (a, b) = (play(), play());
    let bits = |t: &[(mscent_math::Vec3, Pose)]| {
        t.iter()
            .map(|(p, m)| {
                (
                    p.x.to_bits(),
                    p.y.to_bits(),
                    p.z.to_bits(),
                    m.offset.x.to_bits(),
                    m.rot.w.to_bits(),
                )
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(bits(&a), bits(&b));
}

#[test]
fn bad_requests_are_refused_and_change_nothing() {
    let mut game = door_game(v(0.0, 10.0, 0.0));
    let door = game.mover_index("door").unwrap();
    assert_eq!(game.mover_index("nope"), None);
    let ok = open_pose();
    assert_eq!(game.define_pose(9, "a", ok), Err(MoverError::NoSuchMover));
    assert_eq!(
        game.define_pose(door, CLOSED, ok),
        Err(MoverError::BadPoseName)
    );
    for bad in [
        Pose::translated(vd(f64::NAN, 0.0, 0.0)),
        Pose::translated(vd(1.0e30, 0.0, 0.0)),
        Pose {
            pivot: vd(0.0, f64::INFINITY, 0.0),
            ..ok
        },
    ] {
        assert_eq!(game.define_pose(door, "a", bad), Err(MoverError::BadPose));
    }
    assert_eq!(game.move_mover(door, "a", 1.0), Err(MoverError::NoSuchPose));
    assert_eq!(
        game.move_mover(9, CLOSED, 1.0),
        Err(MoverError::NoSuchMover)
    );
    game.define_pose(door, "a", ok).unwrap();
    for bad in [-1.0, f32::NAN, f32::INFINITY, 1.0e9] {
        assert_eq!(game.move_mover(door, "a", bad), Err(MoverError::BadTime));
    }
    assert!(!game.mover(door).unwrap().is_moving());
    for i in 0..MAX_POSES {
        let _ = game.define_pose(door, &format!("p{i}"), ok);
    }
    assert_eq!(
        game.define_pose(door, "extra", ok),
        Err(MoverError::BadPoseName)
    );
    let bad = WhenBlocked::Crush {
        damage_per_second: f32::NAN,
    };
    assert_eq!(game.set_when_blocked(door, bad), Err(MoverError::BadDamage));
}

#[test]
fn a_level_with_no_movers_never_touches_a_mover() {
    let level = open_level(vec![start(v(0.0, 0.0, 0.0), FACE_PLUS_X), far_goal()]);
    let mut game = Game::new(Arena::from_level(&level).unwrap());
    assert!(game.solids.is_empty());
    run(&mut game, 10, &forward());
    assert!(!game.take_events().iter().any(|e| matches!(
        e,
        Event::MoverArrived(_) | Event::MoverBlocked(_) | Event::MoverHit(_)
    )));
}
