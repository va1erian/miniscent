//! Enemy kinds as the game plays them (M19.2): stock stats, a level's own
//! kinds, hit spheres turned by facing, scripts by name, and replays.

use mscent_collide::STEP_HZ;
use mscent_world::kinds::stock_kinds;
use mscent_world::{Entity, Kind, KindStats, Level};

use crate::script_tests::{game_on, level, run};
use crate::test_util::{far_goal, game, open_level, random_trace, start, v};
use crate::weapon::Weapon;
use crate::{Arena, Enemy, EnemyStats, Game, kinds_of, stats_for};

const MANTIS: &str = include_str!("../../../assets/enemies/mantis.msb");

fn mantis(hp: u32) -> Kind {
    Kind {
        name: "mantis".to_string(),
        stats: KindStats {
            hp,
            ..KindStats::DRONE
        },
        blueprint: mscent_world::model::read(MANTIS.as_bytes()).unwrap(),
    }
}

/// `entities` in the open room, with the stock pair and a mantis as kind 2.
fn with_mantis(entities: Vec<Entity>) -> Level {
    let mut lv = open_level(entities);
    lv.kinds = stock_kinds();
    lv.kinds.push(mantis(70));
    lv
}

#[test]
fn stock_stats_are_the_numbers_enemies_always_had() {
    let hz = STEP_HZ as u32;
    let drone = EnemyStats {
        max_hp: 50,
        radius: 0.75,
        speed: 5.0,
        sight_range: 40.0,
        attack_range: 9.0,
        attack_damage: 6,
        attack_cooldown: hz / 2,
        lose_steps: hz * 3,
    };
    let hunter = EnemyStats {
        max_hp: 100,
        speed: 7.0,
        attack_damage: 12,
        attack_cooldown: hz,
        ..drone
    };
    assert_eq!(stats_for(0), drone);
    assert_eq!(stats_for(1), hunter);
    assert_eq!(stats_for(200), drone);
}

#[test]
fn a_level_without_kinds_plays_the_stock_pair_with_bodies() {
    let kinds = kinds_of(&open_level(Vec::new()));
    let names: Vec<&str> = kinds.iter().map(|k| k.name.as_str()).collect();
    assert_eq!(names, ["drone", "hunter"]);
    assert_eq!(
        (kinds[0].stats, kinds[1].stats),
        (stats_for(0), stats_for(1))
    );
    assert!(kinds.iter().all(|k| k.hits.is_empty()));
}

#[test]
fn an_enemy_plays_its_kind() {
    let spawn = Entity::Enemy {
        pos: v(10.0, 0.0, 0.0),
        kind: 2,
    };
    let g = game(&with_mantis(vec![
        start(v(0.0, 0.0, 0.0), 0.0),
        spawn,
        far_goal(),
    ]));
    let e = &g.enemies[0];
    assert_eq!((e.kind, e.hp), (2, 70));
    assert_eq!(e.shot_spheres().count(), 4);
    // The body holds the blades, which reach 3.7 units at the mantis's scale.
    let reach = e
        .shot_spheres()
        .map(|s| (s.centre - e.pos()).length() + s.radius);
    let reach = reach.fold(0.0, f32::max);
    assert!(
        e.body.radius >= reach && reach > 3.5,
        "{} {reach}",
        e.body.radius
    );
}

#[test]
fn a_kind_past_the_table_will_not_load() {
    let mut lv = with_mantis(vec![start(v(0.0, 0.0, 0.0), 0.0), far_goal()]);
    lv.entities.push(Entity::Enemy {
        pos: v(5.0, 0.0, 0.0),
        kind: 3,
    });
    assert!(Arena::from_level(&lv).is_err());
}

/// A mantis at the origin of the open room, facing +X unless turned.
fn mantis_at_origin() -> (Arena, Enemy) {
    let lv = with_mantis(vec![start(v(-15.0, 0.0, 0.0), 0.0), far_goal()]);
    let arena = Arena::from_level(&lv).unwrap();
    let k = &arena.kinds[2];
    let e = Enemy::with_stats(0, v(0.0, 0.0, 0.0), 2, k.stats).with_hits(k.hits.clone());
    (arena, e)
}

/// Where a shot along +X from `from` strikes the enemy, if it does.
fn strike(arena: &Arena, e: &Enemy, from: (f32, f32)) -> Option<f32> {
    let mut enemies = vec![e.clone()];
    let origin = v(-10.0, from.0, from.1);
    let shot = Weapon::default().fire(
        arena.solids(&[]),
        origin,
        v(1.0, 0.0, 0.0),
        &mut enemies,
        &mut [],
    );
    let shot = shot.unwrap();
    shot.hit.map(|_| shot.point.x)
}

#[test]
fn shots_strike_the_hit_spheres_not_the_body() {
    let (arena, e) = mantis_at_origin();
    // Inside the body's sphere, beside every hit sphere: a miss.
    assert!(e.body.radius > 3.0);
    assert_eq!(strike(&arena, &e, (3.0, 0.25)), None);
    // Through the left blade's sphere, at (3, 1.125, 0.25) with radius 0.5.
    let x = strike(&arena, &e, (1.125, 0.25)).expect("the blade is struck");
    assert!((x - 2.5).abs() < 1.0e-3, "{x}");
}

#[test]
fn facing_turns_the_hit_spheres() {
    let (arena, mut e) = mantis_at_origin();
    // A quarter turn left puts the left blade at (-1.125, 3, 0.25).
    e.facing = crate::facing::toward(v(0.0, 1.0, 0.0)).unwrap();
    let x = strike(&arena, &e, (3.0, 0.25)).expect("the turned blade is struck");
    assert!((x + 1.625).abs() < 1.0e-3, "{x}");
    // Where the blade was, the shot now meets the body, turned to (0, 1.75).
    let x = strike(&arena, &e, (1.125, 0.25)).expect("the turned body is struck");
    assert!(x < 0.0, "{x}");
}

#[test]
fn a_stock_enemy_is_struck_by_its_body() {
    let (arena, _) = mantis_at_origin();
    let e = Enemy::new(0, v(0.0, 0.0, 0.0), 0);
    let x = strike(&arena, &e, (0.0, 0.0)).expect("the body is struck");
    assert!((x + 0.75).abs() < 1.0e-3, "{x}");
}

#[test]
fn an_enemy_comes_round_to_face_its_target() {
    let spawn = Entity::Enemy {
        pos: v(12.0, 0.0, 0.0),
        kind: 2,
    };
    let mut g = game(&with_mantis(vec![
        start(v(0.0, 0.0, 0.0), 0.0),
        spawn,
        far_goal(),
    ]));
    run(&mut g, 120);
    let ahead = g.enemies[0].facing.rotate(v(1.0, 0.0, 0.0));
    assert!(ahead.x < -0.95, "faces {ahead:?}");
}

#[test]
fn scripts_find_a_level_kind_by_name_and_build_on_it() {
    let mut lv = level(Vec::new());
    lv.kinds = stock_kinds();
    lv.kinds.push(mantis(70));
    let script = "(defineEnemy: 'big' base: 'mantis') hp: 300.
spawn: 'mantis' at: (x: 6 y: 6 z: 0) tag: 'a'.
spawn: 'big' at: (x: 6 y: -6 z: 0) tag: 'b'.
(defineEnemy: 'third' base: 2) hp: 9.
spawn: 'third' at: (x: -6 y: 6 z: 0) tag: 'c'.";
    let mut g = game_on(&lv, script);
    run(&mut g, 2);
    let got: Vec<(u8, i32, usize)> = g
        .enemies
        .iter()
        .map(|e| (e.kind, e.hp, e.shot_spheres().count()))
        .collect();
    assert_eq!(got, [(2, 70, 4), (2, 300, 4), (2, 9, 4)]);
}

/// What a run leaves that play depends on, bit for bit.
fn state(g: &Game) -> Vec<[u32; 9]> {
    let e = g.enemies.iter().map(|e| {
        let (p, f) = (e.pos(), e.facing);
        [
            p.x,
            p.y,
            p.z,
            f.x,
            f.y,
            f.z,
            f.w,
            e.hp as f32,
            g.player.health as f32,
        ]
        .map(f32::to_bits)
    });
    e.collect()
}

#[test]
fn a_level_with_its_own_kinds_replays_bit_exact() {
    let spawns = (0..3).map(|i| Entity::Enemy {
        pos: v(4.0 + 4.0 * i as f32, 6.0, 0.0),
        kind: i,
    });
    let mut entities = vec![start(v(0.0, 0.0, 0.0), 0.0), far_goal()];
    entities.extend(spawns);
    let lv = with_mantis(entities);
    let trace = random_trace(0x6a3e, 1500);
    let mut a = game(&lv);
    a.run_trace(&trace);
    let mut b = game(&lv);
    b.run_trace(&trace);
    assert_eq!(state(&a), state(&b));
    a.restart();
    a.run_trace(&trace);
    assert_eq!(state(&a), state(&b), "a restart replays the same run");
}
