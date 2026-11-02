use crate::test_util::{
    FACE_PLUS_X, enemy, far_goal, game, goal, open_level, split_level, start, v,
};
use crate::{
    Arena, Event, Input, LoadError, Menu, MenuAction, MenuCommand, Phase, Screen, Shooter, Struck,
    hitscan, ray,
};
use mscent_math::Vec3;
use mscent_world::{Entity, Level};

fn forward_trace(n: usize) -> Vec<Input> {
    let go = Input {
        thrust: v(0.0, 0.0, 1.0),
        ..Input::default()
    };
    vec![go; n]
}

fn win_level() -> Level {
    open_level(vec![
        start(v(-15.0, 0.0, 0.0), FACE_PLUS_X),
        goal(v(15.0, 0.0, 0.0)),
    ])
}

fn lose_level() -> Level {
    open_level(vec![
        start(v(0.0, 0.0, 0.0), 0.0),
        enemy(v(5.0, 0.0, 0.0)),
        far_goal(),
    ])
}

#[test]
fn level_with_every_entity_loads_and_is_placed() {
    let light = Entity::Light {
        pos: v(0.0, 10.0, 0.0),
        color: v(1.0, 0.5, 0.25),
        radius: 12.0,
        intensity: 1.0,
    };
    let level = open_level(vec![
        start(v(1.0, 2.0, 3.0), 0.5),
        light,
        enemy(v(-5.0, 0.0, 0.0)),
        goal(v(9.0, 0.0, 0.0)),
    ]);
    let g = game(&level);
    assert_eq!(g.player.pos(), v(1.0, 2.0, 3.0));
    assert_eq!(g.arena.lights.len(), 1);
    assert_eq!(g.arena.lights[0].color, v(1.0, 0.5, 0.25));
    assert_eq!(g.enemies.len(), 1);
    assert_eq!(g.enemies[0].pos(), v(-5.0, 0.0, 0.0));
    assert_eq!(g.arena.goals[0].pos, v(9.0, 0.0, 0.0));
    assert_eq!(g.phase, Phase::Playing);
}

#[test]
fn unplayable_levels_are_errors_not_panics() {
    let load = |e: Vec<Entity>| Arena::from_level(&open_level(e)).unwrap_err();
    assert_eq!(load(vec![far_goal()]), LoadError::NoPlayerStart);
    assert_eq!(load(vec![start(Vec3::ZERO, 0.0)]), LoadError::NoGoal);
    let nan = start(v(f32::NAN, 0.0, 0.0), 0.0);
    assert_eq!(load(vec![far_goal(), nan]), LoadError::InvalidEntity(1));
    let buried = enemy(v(0.0, 0.0, 100.0));
    let e = load(vec![start(Vec3::ZERO, 0.0), far_goal(), buried]);
    assert_eq!(e, LoadError::SpawnInSolid(2));
    let solid = Level {
        brushes: Vec::new(),
        entities: vec![start(Vec3::ZERO, 0.0), far_goal()],
        ..Default::default()
    };
    assert_eq!(Arena::from_level(&solid), Err(LoadError::SpawnInSolid(0)));
}

#[test]
fn reaching_the_goal_wins_and_freezes_the_game() {
    let mut g = game(&win_level());
    let trace = forward_trace(600);
    g.run_trace(&trace);
    assert_eq!(g.phase, Phase::Won);
    assert!(g.ticks < 600, "won in {} steps", g.ticks);
    assert!(g.take_events().contains(&Event::GoalReached));
    let frozen = (g.ticks, g.player.pos());
    g.run_trace(&trace);
    assert_eq!((g.ticks, g.player.pos()), frozen);
}

#[test]
fn dying_loses() {
    let mut g = game(&lose_level());
    let idle = vec![Input::default(); 3000];
    g.run_trace(&idle);
    assert_eq!(g.phase, Phase::Lost);
    assert!(g.player.is_dead());
    let events = g.take_events();
    assert!(events.contains(&Event::PlayerDied));
    assert!(events.contains(&Event::PlayerHurt(crate::enemy::stats_for(0).attack_damage)));
    assert!(g.hurt_steps <= crate::game::HURT_FLASH_STEPS);
}

#[test]
fn restart_recovers_from_loss_and_win_and_replays_identically() {
    let mut g = game(&lose_level());
    g.run_trace(&vec![Input::default(); 3000]);
    assert_eq!(g.phase, Phase::Lost);
    g.restart();
    assert_eq!(g.phase, Phase::Playing);
    assert_eq!(g.player.health, crate::player::MAX_HEALTH);
    assert_eq!(g.player.pos(), v(0.0, 0.0, 0.0));
    assert_eq!(g.enemies_alive(), 1);
    assert_eq!((g.ticks, g.take_events()), (0, vec![Event::Restarted]));

    let mut w = game(&win_level());
    w.run_trace(&forward_trace(600));
    let first = (w.ticks, w.player);
    w.restart();
    assert_eq!(w.phase, Phase::Playing);
    w.run_trace(&forward_trace(600));
    assert_eq!(w.phase, Phase::Won);
    assert_eq!((w.ticks, w.player), first, "a restart replays the same run");
}

#[test]
fn death_outranks_the_goal() {
    let level = open_level(vec![start(v(0.0, 0.0, 0.0), 0.0), goal(v(0.0, 0.0, 0.0))]);
    let mut g = game(&level);
    g.player.health = 0;
    g.step(&Input::default());
    assert_eq!(g.phase, Phase::Lost);
}

#[test]
fn firing_kills_an_enemy_in_two_hits() {
    let level = open_level(vec![
        start(v(0.0, 0.0, 0.0), 0.0),
        enemy(v(0.0, 0.0, -10.0)),
        far_goal(),
    ]);
    let mut g = game(&level);
    let fire = Input {
        fire: true,
        ..Input::default()
    };
    g.step(&fire);
    assert_eq!(g.enemies[0].hp, 25);
    for _ in 0..10 {
        g.step(&fire);
    }
    assert_eq!(g.enemies[0].hp, 25, "cooldown limits the fire rate");
    for _ in 0..30 {
        g.step(&fire);
    }
    assert_eq!(g.enemies_alive(), 0);
    let events = g.take_events();
    assert!(events.contains(&Event::EnemyKilled(0)));
    assert_eq!(
        g.player.health,
        crate::player::MAX_HEALTH,
        "killed it first"
    );
}

#[test]
fn shots_use_the_movement_trace_and_stop_at_walls() {
    let level = split_level(vec![
        start(v(-5.0, 0.0, 0.0), FACE_PLUS_X),
        enemy(v(8.0, 0.0, 0.0)),
        far_goal(),
    ]);
    let mut g = game(&level);
    let fire = Input {
        fire: true,
        ..Input::default()
    };
    g.run_trace(&[fire; 100]);
    assert_eq!(g.enemies[0].hp, 50, "the divider shields the enemy");
    let (from, dir, range) = (v(-5.0, 0.0, 0.0), v(1.0, 0.0, 0.0), 200.0);
    let shot = hitscan(&g.arena.bsp, from, dir, range, &[]);
    let wall = ray(&g.arena.bsp, from, from + dir * range);
    assert_eq!(
        shot.distance,
        wall.fraction * range,
        "same trace as movement"
    );
    assert!((shot.distance - 4.0).abs() < 0.01);
    assert_eq!(shot.target, None);
}

#[test]
fn hitscan_prefers_the_nearest_target_and_rejects_bad_aim() {
    let g = game(&open_level(vec![start(Vec3::ZERO, 0.0), far_goal()]));
    let s = |z| crate::Sphere {
        centre: v(0.0, 0.0, z),
        radius: 1.0,
    };
    let near_last = [s(-12.0), s(-6.0)];
    let hit = hitscan(
        &g.arena.bsp,
        Vec3::ZERO,
        v(0.0, 0.0, -1.0),
        50.0,
        &near_last,
    );
    assert_eq!(hit.target, Some(1));
    assert!((hit.distance - 5.0).abs() < 1e-3);
    for bad in [Vec3::ZERO, v(f32::NAN, 0.0, 0.0)] {
        assert_eq!(
            hitscan(&g.arena.bsp, Vec3::ZERO, bad, 50.0, &near_last).target,
            None
        );
    }
}

#[test]
fn update_runs_whole_steps_from_wall_clock() {
    let mut g = game(&win_level());
    assert_eq!(g.update(8_333_333, &Input::default()), 0);
    assert_eq!(g.update(1, &Input::default()), 1);
    assert_eq!(g.ticks, 1);
}

#[test]
fn menu_flow_drives_start_pause_end_and_restart() {
    let mut m = Menu::new();
    assert_eq!(m.screen(), Screen::Main);
    assert_eq!(m.press(MenuCommand::Down), MenuAction::None);
    assert_eq!(m.selected(), 1);
    assert_eq!(m.press(MenuCommand::Down), MenuAction::None);
    assert_eq!(m.selected(), 0, "selection wraps");
    assert_eq!(m.press(MenuCommand::Select), MenuAction::Start);
    assert_eq!(m.screen(), Screen::Hidden);
    assert_eq!(m.press(MenuCommand::Back), MenuAction::None);
    assert_eq!(m.screen(), Screen::Paused);
    assert_eq!(m.press(MenuCommand::Back), MenuAction::Resume);

    let mut g = game(&lose_level());
    g.run_trace(&vec![Input::default(); 3000]);
    m.sync_phase(g.phase);
    assert_eq!(m.screen(), Screen::Lost);
    assert_eq!(
        m.press(MenuCommand::Back),
        MenuAction::None,
        "no escaping a loss"
    );
    assert_eq!(m.press(MenuCommand::Select), MenuAction::Restart);
    g.restart();
    m.sync_phase(g.phase);
    assert_eq!((m.screen(), g.phase), (Screen::Hidden, Phase::Playing));
}

fn shots(events: &[Event]) -> Vec<(Shooter, Vec3, Vec3, Vec3, Struck)> {
    events
        .iter()
        .filter_map(|e| match *e {
            Event::Shot {
                shooter,
                origin,
                end,
                normal,
                struck,
            } => Some((shooter, origin, end, normal, struck)),
            _ => None,
        })
        .collect()
}

#[test]
fn a_shot_at_a_wall_reports_the_point_and_the_normal_facing_back() {
    let level = split_level(vec![start(v(-5.0, 0.0, 0.0), FACE_PLUS_X), far_goal()]);
    let mut g = game(&level);
    g.step(&Input {
        fire: true,
        ..Input::default()
    });
    let all = shots(&g.take_events());
    let &[(shooter, origin, end, normal, struck)] = all.as_slice() else {
        panic!("one shot: {all:?}");
    };
    assert_eq!((shooter, struck), (Shooter::Player, Struck::Wall));
    assert_eq!(origin, g.player.pos());
    assert!((end.x - -1.0).abs() < 0.01, "the divider's face: {end:?}");
    assert!((normal - v(-1.0, 0.0, 0.0)).length() < 1e-3, "{normal:?}");
}

#[test]
fn a_shot_at_an_enemy_reports_the_sphere_normal_and_the_enemy() {
    let level = open_level(vec![
        start(v(0.0, 0.0, 0.0), FACE_PLUS_X),
        enemy(v(5.0, 0.0, 0.0)),
        far_goal(),
    ]);
    let mut g = game(&level);
    g.step(&Input {
        fire: true,
        ..Input::default()
    });
    let all = shots(&g.take_events());
    let (_, _, end, normal, struck) = all[0];
    assert_eq!(struck, Struck::Enemy(0));
    assert!(end.x < 5.0, "on the near side of the sphere: {end:?}");
    assert!((normal - v(-1.0, 0.0, 0.0)).length() < 1e-3, "{normal:?}");
}

#[test]
fn an_enemy_killed_after_firing_stops_reporting_that_shot() {
    let level = open_level(vec![
        start(v(0.0, 0.0, 0.0), FACE_PLUS_X),
        enemy(v(5.0, 0.0, 0.0)),
        far_goal(),
    ]);
    let mut g = game(&level);
    let mut waited = 0;
    while g.enemies[0].last_shot.is_none() {
        g.step(&Input::default());
        waited += 1;
        assert!(waited < 2000, "the enemy never fired");
    }
    assert!(g.enemies[0].damage(1000));
    g.take_events();
    g.step(&Input::default());
    assert!(shots(&g.take_events()).is_empty());
    assert_eq!(g.enemies[0].last_shot, None);
}

#[test]
fn shot_events_do_not_change_the_simulation() {
    // The trace replays to the same state whether or not anyone reads events.
    let level = lose_level();
    let (mut read, mut unread) = (game(&level), game(&level));
    for _ in 0..600 {
        read.step(&Input::default());
        read.take_events();
        unread.step(&Input::default());
    }
    assert_eq!(read.player.health, unread.player.health);
    assert_eq!(read.player.pos(), unread.player.pos());
    assert_eq!(read.enemies[0].pos(), unread.enemies[0].pos());
}

#[test]
fn a_mover_brush_stays_out_of_the_world_bsp() {
    use mscent_math::Vec3d;
    use mscent_world::{Brush, MoverGroup, Op};
    let door = Brush::cuboid(
        Vec3d::new(0.0, -1.0, -1.0),
        Vec3d::new(1.0, 1.0, 1.0),
        Op::Add,
        9,
    );
    let mut level = open_level(vec![start(Vec3::new(-3.0, 0.0, 0.0), 0.0), far_goal()]);
    level.brushes.push(door.unwrap());
    let inside = Vec3::new(0.5, 0.0, 0.0).to_f64();
    let solid = Arena::from_level(&level).unwrap();
    assert!(solid.bsp.is_solid(inside), "without a group it is world");
    level.mover_groups.push(MoverGroup {
        name: "door".to_string(),
        tags: Vec::new(),
        brushes: vec![level.brushes.len() as u32 - 1],
    });
    let open = Arena::from_level(&level).unwrap();
    assert!(!open.bsp.is_solid(inside), "in a group it is not");
    level.mover_groups[0].brushes = vec![u32::MAX];
    let refused = Arena::from_level(&level).unwrap_err();
    assert_eq!(
        refused,
        crate::arena::LoadError::Labels(mscent_world::LabelError::BadIndex)
    );
}
