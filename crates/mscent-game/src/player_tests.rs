use crate::player::{MAX_TURN_RATE, THRUST_ACCEL};
use crate::test_util::{far_goal, game, open_level, random_trace, start, v};
use crate::{Input, Player};
use mscent_collide::STEP_DT;
use mscent_math::{Quat, Vec3};
use std::f32::consts::FRAC_PI_2;

fn open_game() -> crate::Game {
    game(&open_level(vec![start(v(0.0, 0.0, 0.0), 0.0), far_goal()]))
}

fn angle_between(a: Vec3, b: Vec3) -> f32 {
    a.dot(b).clamp(-1.0, 1.0).acos()
}

#[test]
fn input_trace_reproduces_motion_exactly() {
    let trace = random_trace(7, 900);
    let (mut a, mut b) = (open_game(), open_game());
    a.run_trace(&trace);
    b.run_trace(&trace);
    assert_eq!(a.player, b.player);
    assert_ne!(a.player.pos(), v(0.0, 0.0, 0.0), "the trace actually moved");
}

#[test]
fn frame_chunking_does_not_change_the_result() {
    let held = Input {
        thrust: v(0.3, 0.1, 1.0),
        pitch: 0.2,
        yaw: -0.4,
        ..Input::default()
    };
    let (mut a, mut b) = (open_game(), open_game());
    // 25 ms and 50 ms frames are exactly 3 and 6 steps at 120 Hz.
    for _ in 0..40 {
        a.update(25_000_000, &held);
    }
    for _ in 0..20 {
        b.update(50_000_000, &held);
    }
    assert_eq!(a.ticks, 120);
    assert_eq!(a.player, b.player);
}

#[test]
fn thrust_goes_forward_in_the_body_frame() {
    let mut g = open_game();
    let held = Input {
        thrust: v(0.0, 0.0, 1.0),
        ..Input::default()
    };
    g.run_trace(&[held; 30]);
    let vel = g.player.body.vel;
    assert!(vel.z < -5.0, "forward is -Z at yaw 0, got {vel:?}");
    assert!(vel.x.abs() < 1e-4 && vel.y.abs() < 1e-4);
    assert!(vel.length() < THRUST_ACCEL, "drag keeps speed bounded");
}

#[test]
fn thrust_on_each_axis_follows_orientation() {
    let g = open_game();
    let mut p = g.player;
    p.orientation = Quat::from_axis_angle(v(0.0, 1.0, 0.0), FRAC_PI_2);
    let right = Input {
        thrust: v(1.0, 0.0, 0.0),
        ..Input::default()
    };
    for _ in 0..20 {
        p.step(&g.arena.bsp, &right);
    }
    // Yawed a quarter turn left, the body's right is world -Z.
    assert!(p.body.vel.z < -2.0 && p.body.vel.x.abs() < 1e-3);
    let up = Input {
        thrust: v(0.0, 1.0, 0.0),
        ..Input::default()
    };
    let mut q = g.player;
    for _ in 0..20 {
        q.step(&g.arena.bsp, &up);
    }
    assert!(q.body.vel.y > 2.0);
}

#[test]
fn motion_and_rotation_have_inertia() {
    let g = open_game();
    let mut p = g.player;
    let go = Input {
        thrust: v(0.0, 0.0, 1.0),
        pitch: 1.0,
        ..Input::default()
    };
    p.step(&g.arena.bsp, &go);
    assert!(p.ang_vel.x < MAX_TURN_RATE, "turn rate ramps up");
    for _ in 0..60 {
        p.step(&g.arena.bsp, &go);
    }
    let (speed, spin) = (p.body.vel.length(), p.ang_vel.length());
    p.step(&g.arena.bsp, &Input::default());
    assert!(
        p.body.vel.length() > 0.8 * speed,
        "eases off, not an instant stop"
    );
    assert!(
        p.ang_vel.length() > 0.5 * spin,
        "keeps turning after release"
    );
}

#[test]
fn pitching_through_ninety_degrees_has_no_gimbal_artifacts() {
    let g = open_game();
    let mut p = g.player;
    let pitch = Input {
        pitch: 1.0,
        ..Input::default()
    };
    for _ in 0..30 {
        p.step(&g.arena.bsp, &pitch);
    }
    let per_step = MAX_TURN_RATE * STEP_DT;
    let mut highest = 0.0_f32;
    for _ in 0..240 {
        let before = p.forward();
        p.step(&g.arena.bsp, &pitch);
        highest = highest.max(p.forward().y);
        let turned = angle_between(before, p.forward());
        assert!((turned - per_step).abs() < 1e-3, "rate changed: {turned}");
        assert!((p.orientation.norm_squared() - 1.0).abs() < 1e-4);
    }
    assert!(highest > 0.9999, "the loop passed straight up");
}

#[test]
fn yaw_and_roll_still_work_when_looking_straight_up() {
    let g = open_game();
    let mut p = g.player;
    p.orientation = Quat::from_axis_angle(v(1.0, 0.0, 0.0), FRAC_PI_2);
    assert!((p.forward() - v(0.0, 1.0, 0.0)).length() < 1e-5);
    let yaw = Input {
        yaw: 1.0,
        ..Input::default()
    };
    for _ in 0..30 {
        p.step(&g.arena.bsp, &yaw);
    }
    let (fwd, up) = (p.forward(), p.up());
    p.step(&g.arena.bsp, &yaw);
    let per_step = MAX_TURN_RATE * STEP_DT;
    assert!((angle_between(fwd, p.forward()) - per_step).abs() < 1e-3);
    assert!(angle_between(up, p.up()) < 1e-3, "yaw turns about body up");
    let roll = Input {
        roll: 1.0,
        ..Input::default()
    };
    for _ in 0..60 {
        p.step(&g.arena.bsp, &roll);
    }
    let m = p.orientation.to_mat3();
    assert!(m.cols[0].dot(m.cols[1]).abs() < 1e-4, "stays orthonormal");
}

#[test]
fn flying_into_a_wall_stops_at_it() {
    let mut g = open_game();
    let held = Input {
        thrust: v(0.0, 0.0, 1.0),
        ..Input::default()
    };
    g.run_trace(&vec![held; 900]);
    let z = g.player.pos().z;
    assert!((-20.0 + 0.5 - 0.01..-19.0).contains(&z), "z = {z}");
}

#[test]
fn hostile_input_never_poisons_the_state() {
    let mut g = open_game();
    let bad = Input {
        thrust: v(f32::NAN, f32::INFINITY, -1e30),
        pitch: f32::NAN,
        yaw: f32::NEG_INFINITY,
        roll: 1e30,
        fire: true,
    };
    g.run_trace(&[bad; 200]);
    assert!(g.player.pos().is_finite());
    assert!((g.player.orientation.norm_squared() - 1.0).abs() < 1e-3);
    let mut p = Player::new(v(0.0, 0.0, 0.0), f32::NAN);
    p.damage(i32::MAX);
    p.damage(i32::MAX);
    assert!(p.is_dead());
}

#[test]
fn mouse_look_accumulates_no_roll_and_is_immediate() {
    let g = open_game();
    let mut p = g.player;
    p.up_ref = Some(v(0.0, 1.0, 0.0));
    for _ in 0..10 {
        p.look(0.15, 0.2);
        p.look(-0.05, -0.1);
    }
    assert!(
        p.right().y.abs() < 1e-4,
        "horizon stays level: {:?}",
        p.right()
    );
    let before = p.forward();
    p.look(0.0, 0.3);
    assert!(angle_between(before, p.forward()) > 0.1, "applied at once");
    assert_eq!(p.ang_vel, Vec3::ZERO, "no angular momentum from the mouse");
}

#[test]
fn a_free_roll_levels_itself_but_a_held_roll_does_not() {
    let g = open_game();
    let mut p = g.player;
    p.up_ref = Some(v(0.0, 1.0, 0.0));
    p.orientation = Quat::from_axis_angle(v(0.0, 0.0, 1.0), 0.8);
    let err = |p: &crate::Player| p.right().y.atan2(p.up().y).abs();
    let start = err(&p);
    assert!(start > 0.7);
    let hold = Input {
        roll: 1.0,
        ..Input::default()
    };
    for _ in 0..10 {
        p.step(&g.arena.bsp, &hold);
    }
    assert!(err(&p) > 0.7, "held roll is not fought");
    for _ in 0..240 {
        p.step(&g.arena.bsp, &Input::default());
    }
    assert!(err(&p) < 0.02, "levelled, still {}", err(&p));
}

#[test]
fn top_speed_is_doorway_friendly_and_release_brakes_quickly() {
    let g = open_game();
    let mut p = g.player;
    let start = p.body.pos;
    let fwd = Input {
        thrust: v(0.0, 0.0, 1.0),
        ..Input::default()
    };
    for _ in 0..240 {
        p.body.pos = start;
        p.step(&g.arena.bsp, &fwd);
    }
    assert!(p.body.vel.length() < 13.0, "{}", p.body.vel.length());
    for _ in 0..60 {
        p.step(&g.arena.bsp, &Input::default());
    }
    assert!(p.body.vel.length() < 0.5, "stops within a second");
}
