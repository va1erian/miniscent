use crate::test_util::{far_goal, game, split_level, start, v};
use crate::{AiState, Enemy, Sphere, line_of_sight, stats_for};
use mscent_math::Vec3;
use mscent_world::Bsp;

fn bsp() -> Bsp {
    let g = game(&split_level(vec![
        start(v(-15.0, 0.0, 0.0), 0.0),
        far_goal(),
    ]));
    g.arena.bsp
}

fn player_at(p: Vec3) -> Sphere {
    Sphere {
        centre: p,
        radius: 0.5,
    }
}

/// Steps until the enemy is in `want`, at most `limit` steps.
fn run_until(e: &mut Enemy, bsp: &Bsp, p: &Sphere, want: AiState, limit: usize) -> bool {
    (0..limit).any(|_| {
        e.step(bsp, p);
        e.state == want
    })
}

const SEEN: Vec3 = Vec3::new(-5.0, 0.0, 0.0);
const BEHIND_WALL: Vec3 = Vec3::new(10.0, 0.0, 0.0);

#[test]
fn line_of_sight_is_blocked_by_the_divider() {
    let b = bsp();
    assert!(line_of_sight(&b, v(-10.0, 0.0, 0.0), SEEN));
    assert!(!line_of_sight(&b, v(-10.0, 0.0, 0.0), BEHIND_WALL));
    assert!(!line_of_sight(&b, v(-10.0, 0.0, 0.0), v(0.0, 0.0, 100.0)));
}

#[test]
fn patrolling_enemy_wanders_without_entering_rock() {
    let b = bsp();
    let far = player_at(v(10.0, 10.0, 10.0));
    let mut e = Enemy::new(3, v(-10.0, 0.0, 0.0), 0);
    let mut farthest = 0.0_f32;
    for _ in 0..3000 {
        e.step(&b, &far);
        assert_eq!(e.state, AiState::Patrol);
        assert!(e.pos().is_finite() && !b.is_solid(e.pos().to_f64()));
        farthest = farthest.max((e.pos() - v(-10.0, 0.0, 0.0)).length());
    }
    assert!(farthest > 3.0, "it actually patrols, moved {farthest}");
}

#[test]
fn sees_chases_then_attacks() {
    let b = bsp();
    let p = player_at(SEEN);
    let mut e = Enemy::new(0, v(-10.0, 0.0, 0.0), 0);
    e.step(&b, &p);
    assert_eq!(e.state, AiState::Chase);
    e.step(&b, &p);
    assert_eq!(e.state, AiState::Attack, "5 units is inside attack range");
}

#[test]
fn seen_but_distant_target_is_chased_not_attacked() {
    let b = bsp();
    let near = player_at(v(-2.5, 10.0, 0.0));
    let mut e = Enemy::new(0, v(-2.5, -15.0, 0.0), 0);
    for _ in 0..30 {
        e.step(&b, &near);
    }
    assert_eq!(e.state, AiState::Chase, "25 units: seen but out of range");
    assert!((e.pos() - near.centre).length() < 25.0, "closing in");
    let beyond = player_at(v(-2.5, 19.0, 0.0));
    let mut idle = Enemy::new(0, v(-19.0, -19.0, -19.0), 0);
    idle.step(&b, &beyond);
    assert_eq!(idle.state, AiState::Patrol, "over 40 units is out of sight");
}

#[test]
fn loses_the_player_behind_cover_then_gives_up() {
    let b = bsp();
    let mut e = Enemy::new(0, v(-10.0, 0.0, 0.0), 0);
    let seen = player_at(SEEN);
    assert!(run_until(&mut e, &b, &seen, AiState::Attack, 5));
    let hidden = player_at(BEHIND_WALL);
    let lose = stats_for(0).lose_steps as usize;
    e.step(&b, &hidden);
    assert_eq!(
        e.state,
        AiState::Chase,
        "out of sight: hunt, do not teleport"
    );
    assert_eq!(e.last_seen, Some(SEEN));
    for _ in 1..lose - 1 {
        e.step(&b, &hidden);
        assert_eq!(e.state, AiState::Chase);
    }
    e.step(&b, &hidden);
    assert_eq!(e.state, AiState::Patrol, "gives up after lose_steps");
    assert_eq!(e.last_seen, None);
}

#[test]
fn reacquires_while_hunting_and_after_giving_up() {
    let b = bsp();
    let (seen, hidden) = (player_at(SEEN), player_at(BEHIND_WALL));
    let mut e = Enemy::new(1, v(-10.0, 0.0, 0.0), 0);
    assert!(run_until(&mut e, &b, &seen, AiState::Attack, 5));
    for _ in 0..60 {
        e.step(&b, &hidden);
    }
    assert_eq!(e.state, AiState::Chase);
    assert!(
        run_until(&mut e, &b, &seen, AiState::Attack, 400),
        "re-seen mid-hunt"
    );
    let lose = stats_for(0).lose_steps as usize;
    for _ in 0..lose + 5 {
        e.step(&b, &hidden);
    }
    assert_eq!(e.state, AiState::Patrol);
    assert!(
        run_until(&mut e, &b, &seen, AiState::Chase, 2),
        "re-seen after patrol"
    );
}

#[test]
fn attack_hits_in_the_open_and_never_through_the_wall() {
    let b = bsp();
    let mut e = Enemy::new(0, v(-10.0, 0.0, 0.0), 0);
    let seen = player_at(SEEN);
    let dealt: i32 = (0..300).filter_map(|_| e.step(&b, &seen)).sum();
    assert!(dealt >= 40 && dealt % 10 == 0, "dealt {dealt}");
    let mut e = Enemy::new(0, v(-10.0, 0.0, 0.0), 0);
    let hidden = player_at(BEHIND_WALL);
    let dealt: i32 = (0..300).filter_map(|_| e.step(&b, &hidden)).sum();
    assert_eq!(dealt, 0);
}

#[test]
fn same_seed_same_wander_and_death_is_final() {
    let b = bsp();
    let far = player_at(v(10.0, 10.0, 10.0));
    let run = || {
        let mut e = Enemy::new(5, v(-10.0, 2.0, 0.0), 1);
        for _ in 0..1000 {
            e.step(&b, &far);
        }
        e.pos()
    };
    assert_eq!(run(), run());
    let mut e = Enemy::new(0, v(-10.0, 0.0, 0.0), 0);
    assert!(!e.damage(49));
    assert!(e.damage(10));
    assert!(!e.damage(10), "already dead");
    assert_eq!(e.step(&b, &player_at(SEEN)), None);
    assert_eq!(e.state, AiState::Dead);
    assert_eq!(stats_for(200), stats_for(0), "unknown kinds fall back");
}
