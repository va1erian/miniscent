//! Each level scripting use case, played through the real `Game::step`.

use crate::mover::CLOSED;
use crate::test_util::{FACE_PLUS_X, enemy, open_level, start, v};
use crate::{Arena, Event, Game, Input, Phase};
use mscent_collide::Pose;
use mscent_math::{Vec3, Vec3d};
use mscent_script::Source;
use mscent_world::{Brush, Entity, EntityLabel, Level, MoverGroup, Op};

const DOORS: &str = include_str!("../../../assets/scripts/lib/doors.st");

fn vd(x: f64, y: f64, z: f64) -> Vec3d {
    Vec3d::new(x, y, z)
}

pub(crate) fn zone(x: f32) -> Entity {
    Entity::Zone {
        pos: v(x, 10.0, 0.0),
        half: v(2.0, 2.0, 2.0),
    }
}

fn marker(x: f32, y: f32) -> Entity {
    Entity::Marker {
        pos: v(x, y, 0.0),
        yaw: 0.0,
    }
}

fn slab(x: f64, order: u32) -> Brush {
    Brush::cuboid(vd(x, -3.0, -3.0), vd(x + 1.0, 3.0, 3.0), Op::Add, order).unwrap()
}

/// The open room with the player at the origin facing +X, a door across x = 14
/// and a second mover across x = -14, and these named entities.
pub(crate) fn level(named: Vec<(Entity, &str, &[&str])>) -> Level {
    let mut lv = open_level(vec![start(v(0.0, 0.0, 0.0), FACE_PLUS_X)]);
    lv.brushes.push(slab(14.0, 90));
    lv.brushes.push(slab(-14.0, 91));
    for (name, brush) in [("door", 1), ("lift", 2)] {
        lv.mover_groups.push(MoverGroup {
            name: name.to_string(),
            tags: Vec::new(),
            brushes: vec![brush],
        });
    }
    for (entity, name, tags) in named {
        lv.labels.push(EntityLabel {
            entity: lv.entities.len() as u32,
            name: (!name.is_empty()).then(|| name.to_string()),
            tags: tags.iter().map(|t| t.to_string()).collect(),
        });
        lv.entities.push(entity);
    }
    lv
}

pub(crate) fn game_on(lv: &Level, script: &str) -> Game {
    let mut game = Game::new(Arena::from_scripted_level(lv).unwrap());
    let sources = [
        Source {
            name: "doors.st",
            text: DOORS,
        },
        Source {
            name: "level.st",
            text: script,
        },
    ];
    game.load_script(&sources).expect("the script loads");
    game
}

pub(crate) fn run(game: &mut Game, steps: usize) {
    for _ in 0..steps {
        game.step(&Input::default());
    }
}

pub(crate) fn no_errors(game: &mut Game) {
    assert_eq!(game.take_script_errors(), []);
}

fn door(game: &Game) -> Option<&str> {
    let i = game.mover_index("door").unwrap();
    game.mover(i).unwrap().pose_name()
}

fn switches() -> Vec<(Entity, &'static str, &'static [&'static str])> {
    vec![
        (marker(4.0, 0.0), "vault_switch_1", &[]),
        (marker(7.0, 0.0), "vault_switch_2", &[]),
        (marker(10.0, 0.0), "vault_switch_3", &[]),
    ]
}

const SWITCH_SCRIPT: &str = "
    sw := defineEnemy: 'switch' base: 0.
    sw hp: 25.
    sw speed: 0.
    sw damage: 0.
    vault := mover: 'door'.
    vault pose: 'open' move: (x: 0 y: 0 z: 20).
    (markers: 'vault_switch') do: [:m | spawn: 'switch' at: m tag: 'vault_switch'].
    switches := tagged: 'vault_switch'.
    switches onEachDestroyed: [ say: switches alive , ' left' ].
    switches onAllDestroyed: [
        vault moveTo: 'open' in: 0.5.
        say: 'Vault unlocked'.
    ].";

#[test]
fn shooting_several_things_opens_a_door() {
    let mut game = game_on(&level(switches()), SWITCH_SCRIPT);
    assert_eq!(game.enemies.len(), 3);
    let fire = Input {
        fire: true,
        ..Input::default()
    };
    // Two of three down: still shut.
    for _ in 0..40 {
        game.step(&fire);
    }
    assert_eq!(game.take_messages(), ["2 left", "1 left"]);
    assert_eq!(door(&game), Some(CLOSED));
    for _ in 0..200 {
        game.step(&fire);
    }
    no_errors(&mut game);
    assert_eq!(game.take_messages(), ["0 left", "Vault unlocked"]);
    assert_eq!(door(&game), Some("open"));
    assert_eq!(game.enemies_alive(), 0);
}

#[test]
fn a_key_door_needs_the_key_and_closes_behind_the_player() {
    let named: Vec<(Entity, &str, &[&str])> = vec![
        (zone(14.0), "door_area", &[]),
        (zone(-14.0), "key_room", &[]),
    ];
    let src = "
        (zone: 'key_room') onEnter: [ player give: 'red'. say: 'Red keycard' ].
        autoDoor: 'door' travel: (x: 0 y: 0 z: 20) key: 'red'.";
    let mut game = game_on(&level(named), src);
    let visit = |game: &mut Game, x: f32| {
        game.player.body.pos = v(x, 10.0, 0.0);
        run(game, 2);
    };
    visit(&mut game, 14.0);
    assert_eq!(game.take_messages(), ["Needs the red keycard"]);
    run(&mut game, 120);
    assert_eq!(door(&game), Some(CLOSED), "no key, no door");
    visit(&mut game, 0.0);
    visit(&mut game, -14.0);
    assert!(game.has_item("red"));
    visit(&mut game, 14.0);
    run(&mut game, 90);
    assert_eq!(door(&game), Some("open"));
    visit(&mut game, 0.0);
    run(&mut game, 120);
    assert_eq!(door(&game), Some(CLOSED), "it closes behind the player");
    no_errors(&mut game);
}

#[test]
fn a_hinge_a_lift_and_a_crusher_are_all_one_mover() {
    let named: Vec<(Entity, &str, &[&str])> = vec![(marker(14.0, -3.0), "hinge", &[])];
    let src = "
        hinged := mover: 'door'.
        hinged pose: 'open' turn: (x: 0 y: 0 z: 1) degrees: 90 about: (marker: 'hinge').
        hinged moveTo: 'open' in: 0.5.
        lift := mover: 'lift'.
        lift pose: 'top' move: (x: 0 y: 0 z: 10).
        lift whenBlocked: 'hold'.
        fork: [
            2 timesRepeat: [
                lift moveTo: 'top' in: 0.5.
                waitArrived: lift.
                say: lift poseNow.
                wait: 0.25.
                lift moveTo: 'closed' in: 0.5.
                waitArrived: lift.
                say: lift poseNow.
                wait: 0.25.
            ].
        ].";
    let mut game = game_on(&level(named), src);
    run(&mut game, 70);
    assert_eq!(door(&game), Some("open"));
    // The hinge turned it round, so its old place is free.
    let pose = *game.mover(0).unwrap().pose();
    assert!(Pose::IDENTITY.angle_to(&pose) > 1.5, "{pose:?}");
    run(&mut game, 300);
    no_errors(&mut game);
    let said = game.take_messages();
    assert_eq!(said, ["top", "closed", "top", "closed"]);
}

const AMBUSH: &str = "
    way_out := mover: 'door'.
    way_out pose: 'open' move: (x: 0 y: 0 z: 20).
    (zone: 'hall') onceEnter: [
        wait: 1.
        spawnAll: 'drone' at: (markers: 'wave_') tag: 'wave1'.
        say: 'wave one'.
        waitDestroyed: (tagged: 'wave1').
        wait: 0.5.
        spawn: 'hunter' at: (marker: 'wave_b') tag: 'wave2'.
        say: 'wave two'.
        waitDestroyed: (tagged: 'wave2').
        way_out moveTo: 'open' in: 0.25.
        say: 'clear'.
    ].";

fn ambush_level() -> Level {
    level(vec![
        (zone(0.0), "hall", &[]),
        (marker(-5.0, 12.0), "wave_a", &[]),
        (marker(-5.0, 14.0), "wave_b", &[]),
    ])
}

fn kill_all(game: &mut Game) {
    game.enemies.iter_mut().for_each(|e| {
        e.damage(100_000);
    });
}

#[test]
fn enemies_spawn_in_waves_and_the_door_waits_for_them() {
    let mut game = game_on(&ambush_level(), AMBUSH);
    assert_eq!(game.enemies.len(), 0, "nothing until the zone");
    game.player.body.pos = v(0.0, 10.0, 0.0);
    run(&mut game, 60);
    assert_eq!(game.enemies.len(), 0, "the wait: 1 has not passed");
    run(&mut game, 70);
    assert_eq!(game.enemies.len(), 2);
    assert_eq!(game.take_messages(), ["wave one"]);
    run(&mut game, 120);
    assert_eq!(game.enemies.len(), 2, "it waits for the wave");
    kill_all(&mut game);
    run(&mut game, 100);
    assert_eq!(game.enemies.len(), 3);
    assert_eq!(game.take_messages(), ["wave two"]);
    assert_eq!(door(&game), Some(CLOSED));
    kill_all(&mut game);
    run(&mut game, 60);
    assert_eq!(door(&game), Some("open"));
    assert_eq!(game.take_messages(), ["clear"]);
    no_errors(&mut game);
}

#[test]
fn a_group_can_be_disabled_and_enabled() {
    let named: Vec<(Entity, &str, &[&str])> = vec![
        (enemy(v(5.0, 0.0, 0.0)), "guard", &["guards"]),
        (zone(-14.0), "off_switch", &[]),
        (zone(14.0), "on_switch", &[]),
    ];
    let src = "
        (zone: 'off_switch') onEnter: [ (tagged: 'guards') disable ].
        (zone: 'on_switch') onEnter: [ (enemy: 'guard') enable ].";
    let mut game = game_on(&level(named), src);
    game.player.body.pos = v(-14.0, 10.0, 0.0);
    run(&mut game, 2);
    let (hp, at) = (game.player.health, game.enemies[0].pos());
    game.player.body.pos = v(5.0, 2.0, 0.0);
    run(&mut game, 240);
    assert_eq!(game.player.health, hp, "a disabled guard does not shoot");
    assert_eq!(game.enemies[0].pos(), at, "or move");
    game.player.body.pos = v(14.0, 10.0, 0.0);
    run(&mut game, 2);
    game.player.body.pos = v(5.0, 2.0, 0.0);
    run(&mut game, 240);
    assert!(game.player.health < hp, "enabled, it does");
    no_errors(&mut game);
}

#[test]
fn a_handler_is_told_its_subject_if_it_asks() {
    let named: Vec<(Entity, &str, &[&str])> = vec![
        (enemy(v(3.0, 0.0, 0.0)), "", &["pack"]),
        (enemy(v(3.0, 2.0, 0.0)), "", &["pack"]),
    ];
    let src = "(tagged: 'pack') onEachDestroyed: [:one | say: one alive , ' ' , one hp].";
    let mut game = game_on(&level(named), src);
    let fire = Input {
        fire: true,
        ..Input::default()
    };
    for _ in 0..60 {
        game.step(&fire);
    }
    no_errors(&mut game);
    assert_eq!(game.take_messages(), ["0 0"]);
}

#[test]
fn a_script_ends_the_level_and_needs_no_goal() {
    let named: Vec<(Entity, &str, &[&str])> =
        vec![(zone(0.0), "exit", &[]), (zone(-14.0), "pit", &[])];
    let lv = level(named);
    assert!(Arena::from_level(&lv).is_err(), "no goal");
    let src = "(zone: 'exit') onEnter: [ win ]. (zone: 'pit') onEnter: [ lose ].";
    let mut game = game_on(&lv, src);
    run(&mut game, 5);
    assert_eq!(game.phase, Phase::Playing);
    assert_eq!(game.goal_distance(), f32::MAX);
    game.player.body.pos = v(-14.0, 10.0, 0.0);
    run(&mut game, 2);
    assert_eq!(game.phase, Phase::Lost);
    assert!(game.take_events().contains(&Event::ScriptLost));
    game.restart();
    game.player.body.pos = v(0.0, 10.0, 0.0);
    run(&mut game, 2);
    assert_eq!(game.phase, Phase::Won);
    assert!(game.take_events().contains(&Event::ScriptWon));
}

#[test]
fn once_enter_fires_once_and_on_enter_every_time() {
    let named: Vec<(Entity, &str, &[&str])> = vec![(zone(0.0), "hall", &[])];
    let src = "(zone: 'hall') onceEnter: [ say: 'once' ].
        (zone: 'hall') onEnter: [ say: 'each' ].";
    let mut game = game_on(&level(named), src);
    for _ in 0..3 {
        game.player.body.pos = v(0.0, 10.0, 0.0);
        run(&mut game, 2);
        game.player.body.pos = v(0.0, 0.0, 0.0);
        run(&mut game, 2);
    }
    no_errors(&mut game);
    assert_eq!(game.take_messages(), ["once", "each", "each", "each"]);
}

#[test]
fn restart_starts_the_script_over() {
    let mut game = game_on(&ambush_level(), AMBUSH);
    game.player.body.pos = v(0.0, 10.0, 0.0);
    run(&mut game, 130);
    assert_eq!(game.enemies.len(), 2);
    game.take_messages();
    game.restart();
    assert_eq!(game.enemies.len(), 0);
    assert_eq!(game.player.pos(), v(0.0, 0.0, 0.0));
    game.player.body.pos = v(0.0, 10.0, 0.0);
    run(&mut game, 130);
    assert_eq!(game.enemies.len(), 2, "the once handler fires again");
    assert_eq!(game.take_messages(), ["wave one"]);
}

fn play_for_replay() -> (String, Vec<u32>) {
    let src = "
        n := 0.
        every: 0.25 do: [
            n := n + 1.
            say: n , ' ' , (randomFrom: 0 to: 100).
            n = 8 ifTrue: [ spawn: 'drone' at: (x: 5 y: 5 z: 0) tag: 'late' ].
        ].
        (tagged: 'late') onAllDestroyed: [ say: 'late gone' ].
        lift := mover: 'lift'.
        lift pose: 'up' move: (x: 0 y: 0 z: 5).
        lift moveTo: 'up' in: 1.";
    let mut game = game_on(&level(Vec::new()), src);
    let (mut text, mut bits) = (String::new(), Vec::new());
    let input = Input {
        thrust: v(0.3, 0.2, 0.5),
        yaw: 0.1,
        fire: true,
        ..Input::default()
    };
    for _ in 0..1200 {
        game.step(&input);
        let p: Vec3 = game.player.pos();
        bits.extend([
            p.x.to_bits(),
            p.y.to_bits(),
            p.z.to_bits(),
            game.player.health as u32,
        ]);
        text += &format!("{:?}{:?}", game.take_messages(), game.take_events());
    }
    text += &format!("{:?}", game.take_script_errors());
    (text, bits)
}

#[test]
fn a_scripted_run_replays_byte_for_byte() {
    let (a, b) = (play_for_replay(), play_for_replay());
    assert!(
        a.0.contains("late gone") || a.0.contains('8'),
        "{}",
        &a.0[..200]
    );
    assert_eq!(a.0, b.0);
    assert_eq!(a.1, b.1);
}

#[test]
fn a_clone_mid_wait_plays_on_the_same() {
    let mut game = game_on(&ambush_level(), AMBUSH);
    game.player.body.pos = v(0.0, 10.0, 0.0);
    run(&mut game, 30);
    let mut copy = game.clone();
    run(&mut game, 200);
    run(&mut copy, 200);
    assert_eq!(game.enemies.len(), copy.enemies.len());
    assert_eq!(game.take_messages(), copy.take_messages());
    assert_eq!(game.player.pos(), copy.player.pos());
}
