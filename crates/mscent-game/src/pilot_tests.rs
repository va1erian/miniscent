use crate::pilot::{EYE_RISE, JET_FUEL, PILOT_RADIUS, RUN_SPEED, STEP_HEIGHT, heading_of};
use crate::test_util::{start, v};
use crate::{Arena, Event, Game, Input, Pilot, ScaleGuide};
use mscent_collide::STEP_HZ;
use mscent_math::{Quat, Vec3, Vec3d};
use mscent_world::{Brush, Entity, Level, Op};

/// Steps in `s` seconds.
fn secs(s: f32) -> usize {
    (s * STEP_HZ as f32) as usize
}

/// Corners are on the 1/8 m grid, which brushes snap to.
fn cuboid(lo: [f64; 3], hi: [f64; 3], op: Op, order: u32) -> Brush {
    let d = |p: [f64; 3]| Vec3d::new(p[0], p[1], p[2]);
    Brush::cuboid(d(lo), d(hi), op, order).unwrap()
}

/// A hall with its floor at z = 0 and its ceiling at 40, and `extra` solids.
fn hall(extra: &[([f64; 3], [f64; 3])], entities: Vec<Entity>) -> Game {
    let mut brushes = vec![cuboid(
        [-30.0, -30.0, 0.0],
        [30.0, 30.0, 40.0],
        Op::Subtract,
        0,
    )];
    for (i, (lo, hi)) in extra.iter().enumerate() {
        brushes.push(cuboid(*lo, *hi, Op::Add, i as u32 + 1));
    }
    let level = Level {
        brushes,
        entities,
        ..Default::default()
    };
    Game::new(Arena::from_level(&level).unwrap())
}

fn plain_hall(extra: &[([f64; 3], [f64; 3])]) -> Game {
    hall(extra, vec![start(v(0.0, 0.0, 6.0), 0.0), far_goal_up()])
}

fn far_goal_up() -> Entity {
    Entity::Goal {
        pos: v(-25.0, -25.0, 10.0),
        radius: 1.0,
    }
}

/// The game with the pilot out at `feet`, looking along +X.
fn walker(mut g: Game, feet: Vec3) -> Game {
    let mut p = Pilot::new(feet, Quat::IDENTITY);
    p.face(v(1.0, 0.0, 0.0));
    g.pilot = Some(p);
    g
}

fn pilot(g: &Game) -> &Pilot {
    g.pilot.as_ref().expect("on foot")
}

fn run(g: &mut Game, input: Input, steps: usize) {
    for _ in 0..steps {
        g.step(&input);
    }
}

fn walk() -> Input {
    Input {
        thrust: v(0.0, 0.0, 1.0),
        ..Input::default()
    }
}

fn jump() -> Input {
    Input {
        thrust: v(0.0, 1.0, 0.0),
        ..Input::default()
    }
}

fn board() -> Input {
    Input {
        board: true,
        ..Input::default()
    }
}

#[test]
fn the_pilot_falls_and_stands_on_the_floor() {
    let mut g = walker(plain_hall(&[]), v(0.0, 0.0, 4.0));
    run(&mut g, Input::default(), secs(1.5));
    let p = pilot(&g);
    assert!(p.grounded, "standing after the fall");
    assert!(
        (p.pos().z - PILOT_RADIUS).abs() < 0.01,
        "feet on the floor: {}",
        p.pos().z
    );
    assert!((p.eye().z - (PILOT_RADIUS + EYE_RISE)).abs() < 0.01);
}

#[test]
fn walking_reaches_run_speed_and_friction_stops_it() {
    let mut g = walker(plain_hall(&[]), v(-20.0, 0.0, 1.0));
    run(&mut g, Input::default(), secs(0.5));
    run(&mut g, walk(), secs(1.0));
    let speed = pilot(&g).body.vel.length();
    assert!((speed - RUN_SPEED).abs() < 0.05, "runs at {speed}");
    assert!(pilot(&g).body.vel.x > 5.9, "along where it looks");
    run(&mut g, Input::default(), secs(0.5));
    assert!(
        pilot(&g).body.vel.length() < 1e-3,
        "friction stops a released walk"
    );
}

#[test]
fn a_jump_rises_about_a_metre_and_lands() {
    let mut g = walker(plain_hall(&[]), v(0.0, 0.0, 1.0));
    run(&mut g, Input::default(), secs(0.5));
    let floor = pilot(&g).pos().z;
    let mut top = floor;
    g.step(&jump());
    for _ in 0..secs(1.0) {
        g.step(&Input::default());
        top = top.max(pilot(&g).pos().z);
    }
    let guide = ScaleGuide::new(1.0);
    assert!(
        (top - floor - guide.jump).abs() < 0.06,
        "rose {}",
        top - floor
    );
    assert!(pilot(&g).grounded);
}

#[test]
fn a_held_jump_is_one_jump_until_released() {
    let mut g = walker(plain_hall(&[]), v(0.0, 0.0, 1.0));
    run(&mut g, Input::default(), secs(0.5));
    // Out of fuel, so holding on can only show as a second jump.
    g.pilot.as_mut().unwrap().fuel = 0.0;
    run(&mut g, jump(), secs(2.0));
    assert!(pilot(&g).grounded, "back down, not hopping");
}

#[test]
fn a_step_is_walked_up_and_a_wall_is_not() {
    let low = 0.375;
    assert!(low < STEP_HEIGHT as f64);
    let mut g = walker(
        plain_hall(&[([2.0, -5.0, 0.0], [8.0, 5.0, low])]),
        v(-2.0, 0.0, 1.0),
    );
    run(&mut g, Input::default(), secs(0.3));
    run(&mut g, walk(), secs(1.0));
    let p = pilot(&g);
    assert!(p.pos().x > 3.0, "walked onto the step: x {}", p.pos().x);
    assert!((p.pos().z - (low as f32 + PILOT_RADIUS)).abs() < 0.02);

    let high = 0.75;
    assert!(high > STEP_HEIGHT as f64);
    let mut g = walker(
        plain_hall(&[([2.0, -5.0, 0.0], [8.0, 5.0, high])]),
        v(-2.0, 0.0, 1.0),
    );
    run(&mut g, Input::default(), secs(0.3));
    run(&mut g, walk(), secs(1.0));
    assert!(
        pilot(&g).pos().x < 2.0 - PILOT_RADIUS + 0.01,
        "stopped at the ledge"
    );
}

#[test]
fn a_low_ceiling_stops_the_head_and_a_tall_one_does_not() {
    // A beam across the way, its underside 1.375 m up: under the pilot's height.
    let beam = |under: f64| ([2.0, -10.0, under], [3.0, 10.0, 40.0]);
    let mut g = walker(plain_hall(&[beam(1.375)]), v(-2.0, 0.0, 1.0));
    run(&mut g, Input::default(), secs(0.3));
    run(&mut g, walk(), secs(1.5));
    assert!(
        pilot(&g).pos().x < 2.0,
        "too low to walk under: x {}",
        pilot(&g).pos().x
    );

    let mut g = walker(plain_hall(&[beam(2.25)]), v(-2.0, 0.0, 1.0));
    run(&mut g, Input::default(), secs(0.3));
    run(&mut g, walk(), secs(1.5));
    assert!(pilot(&g).pos().x > 4.0, "walked under a tall one");
}

#[test]
fn the_jetpack_climbs_past_a_jump_and_the_tank_refills() {
    let mut g = walker(plain_hall(&[]), v(0.0, 0.0, 1.0));
    run(&mut g, Input::default(), secs(0.5));
    let floor = pilot(&g).pos().z;
    let mut top = floor;
    for _ in 0..secs(3.0) {
        g.step(&jump());
        top = top.max(pilot(&g).pos().z);
    }
    let guide = ScaleGuide::new(1.0);
    assert!(top - floor > 3.0 * guide.jump, "flew {} m", top - floor);
    assert!(pilot(&g).fuel < JET_FUEL * 0.5, "it burned fuel");
    run(&mut g, Input::default(), secs(6.0));
    assert!(pilot(&g).grounded);
    assert_eq!(pilot(&g).fuel, JET_FUEL, "full again after a rest");
}

#[test]
fn the_guide_jetpack_climb_matches_the_game() {
    let mut g = walker(plain_hall(&[]), v(0.0, 0.0, 1.0));
    run(&mut g, Input::default(), secs(0.5));
    let floor = pilot(&g).pos().z;
    let mut top = floor;
    // Held from the ground: the jump, then the jetpack past its peak speed.
    for _ in 0..secs(5.0) {
        g.step(&jump());
        top = top.max(pilot(&g).pos().z);
    }
    let climb = ScaleGuide::new(1.0).jet_climb;
    assert!(
        (top - floor - climb).abs() < climb * 0.05,
        "game {} guide {climb}",
        top - floor
    );
}

/// The stock ship at rest just above the floor, or high in the air.
fn ship_game(height: f32) -> Game {
    hall(&[], vec![start(v(0.0, 0.0, height), 0.0), far_goal_up()])
}

#[test]
fn the_pilot_leaves_a_landed_ship_and_boards_it_again() {
    let mut g = ship_game(0.8);
    assert!(g.ship_landed());
    g.step(&board());
    assert!(g.on_foot());
    assert!(g.take_events().contains(&Event::Disembarked));
    run(&mut g, Input::default(), secs(0.5));
    let p = pilot(&g);
    assert!(p.grounded, "stepped out onto the floor");
    assert!((p.pos() - g.player.pos()).length() > g.player.body.radius + PILOT_RADIUS);
    let parked = g.player.pos();
    run(&mut g, walk(), secs(0.3));
    assert_eq!(g.player.pos(), parked, "the parked ship stays put");
    // A held key is one press: release it, then press again.
    g.step(&board());
    assert!(!g.on_foot(), "boarded beside the ship");
    assert!(g.take_events().contains(&Event::Boarded));
}

#[test]
fn a_flying_ship_cannot_be_left_and_a_far_one_cannot_be_boarded() {
    let mut g = ship_game(8.0);
    assert!(!g.ship_landed());
    g.step(&board());
    assert!(!g.on_foot());
    assert!(g.take_events().contains(&Event::BoardRefused));

    let mut g = ship_game(0.8);
    g.step(&board());
    run(&mut g, Input::default(), 1);
    run(&mut g, walk(), secs(2.0));
    g.step(&board());
    assert!(g.on_foot(), "too far to board");
    assert!(g.take_events().contains(&Event::BoardRefused));
}

#[test]
fn a_bigger_ship_has_a_bigger_sphere_and_flies_faster() {
    let mut level = Level {
        brushes: vec![cuboid(
            [-60.0, -60.0, 0.0],
            [60.0, 60.0, 40.0],
            Op::Subtract,
            0,
        )],
        entities: vec![start(v(0.0, 0.0, 20.0), 0.0), far_goal_up()],
        ..Default::default()
    };
    level.entities.push(Entity::Scale {
        pos: v(0.0, 0.0, 1.0),
        ship: 4.0,
    });
    let mut g = Game::new(Arena::from_level(&level).unwrap());
    assert_eq!(g.player.body.radius, 2.0);
    // A bare game's ship looks down; its right, +X, is level.
    let thrust = Input {
        thrust: v(1.0, 0.0, 0.0),
        ..Input::default()
    };
    // Long enough to reach top speed, short of the wall 58 m away.
    run(&mut g, thrust, secs(1.5));
    let speed = g.player.body.vel.length();
    assert!(
        (speed - ScaleGuide::new(4.0).ship_speed).abs() < 1.0,
        "speed {speed}"
    );
}

#[test]
fn a_replay_through_landing_walking_and_boarding_is_exact() {
    let mut trace = vec![board()];
    trace.extend([Input::default(); 30]);
    trace.extend([walk(); 60]);
    trace.extend([jump(); 90]);
    trace.extend([Input::default(); 120]);
    let (mut a, mut b) = (ship_game(0.8), ship_game(0.8));
    a.run_trace(&trace);
    b.run_trace(&trace);
    assert!(a.on_foot());
    assert_eq!(a.pilot, b.pilot);
    assert_eq!(a.player, b.player);
}

#[test]
fn enemies_and_shots_follow_the_pilot() {
    let mut g = ship_game(0.8);
    g.step(&board());
    let p = pilot(&g);
    assert_eq!(g.actor_pos(), p.chest());
    assert_eq!(g.aim().0, p.eye());
    assert!(g.actor_radius() > PILOT_RADIUS);
}

#[test]
fn a_heading_is_level_and_upright() {
    let tilted = Quat::from_axis_angle(v(0.3, 0.8, 0.5).normalized().unwrap(), 1.1);
    let q = heading_of(tilted);
    let fwd = q.rotate(v(0.0, 0.0, -1.0));
    let up = q.rotate(v(0.0, 1.0, 0.0));
    assert!(fwd.z.abs() < 1e-5 && (up.z - 1.0).abs() < 1e-5);
}

#[test]
fn look_never_tips_over_the_top() {
    let mut p = Pilot::new(v(0.0, 0.0, 0.0), Quat::IDENTITY);
    for _ in 0..50 {
        p.look(0.2, 0.05);
    }
    let up = p.orientation.rotate(v(0.0, 1.0, 0.0));
    assert!(up.z > 0.0, "still upright");
    assert!(p.forward().z < 1.0 && p.forward().z > 0.99);
}

#[test]
fn the_guide_tells_the_ship_and_the_pilot_apart() {
    let big = ScaleGuide::new(5.0);
    assert_eq!(big.ship_radius, 2.5);
    // The valley's base door, 3 by 2.5 m.
    assert!(big.pilot_fits(3.0, 2.5) && !big.ship_fits(3.0, 2.5));
    assert!(big.ship_fits(6.0, 6.0));
    assert!(!big.pilot_fits(0.6, 2.0) && !big.pilot_fits(1.0, 1.7));
    let stock = ScaleGuide::new(1.0);
    assert!(stock.ship_fits(1.0, 1.0) && !stock.pilot_fits(1.0, 1.0));
    assert!(big.to_string().contains("keeps the ship out"));
    assert!(stock.to_string().contains("the ship is the smaller"));
}
