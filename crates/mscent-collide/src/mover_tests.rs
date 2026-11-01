use crate::test_util::{room, room_brush, v, vd};
use crate::{
    Body, MoverShape, MoverSolid, Pose, Push, Solids, push_mover, slide_move, sweep_sphere,
};
use mscent_math::Vec3;
use mscent_world::{Brush, Op, compile};
use std::f32::consts::FRAC_PI_2;
use std::sync::Arc;

/// A slab across x = 5, y and z from -2 to 2, 1 thick.
fn door() -> MoverSolid {
    let b = Brush::cuboid(vd(5.0, -2.0, -2.0), vd(6.0, 2.0, 2.0), Op::Add, 0).unwrap();
    MoverSolid::new(Arc::new(MoverShape::from_brushes(std::iter::once(&b))))
}

#[test]
fn a_sweep_stops_at_a_mover_and_names_it() {
    let bsp = room();
    let movers = [door()];
    let tr = sweep_sphere(
        Solids::new(&bsp, &movers),
        v(0.0, 0.0, 0.0),
        v(10.0, 0.0, 0.0),
        0.5,
    );
    assert!(tr.hit);
    assert_eq!(tr.mover, Some(0));
    assert!((tr.end.x - 4.5).abs() < 0.01, "{tr:?}");
    assert_eq!(tr.normal, v(-1.0, 0.0, 0.0));
    let wall = sweep_sphere(
        Solids::new(&bsp, &movers),
        v(0.0, 0.0, 0.0),
        v(-30.0, 0.0, 0.0),
        0.5,
    );
    assert!(wall.hit);
    assert_eq!(wall.mover, None);
}

#[test]
fn a_sweep_passes_a_mover_once_it_has_moved_away() {
    let bsp = room();
    let mut movers = [door()];
    movers[0].set_pose(&Pose::translated(vd(0.0, 0.0, 10.0)));
    let tr = sweep_sphere(
        Solids::new(&bsp, &movers),
        v(0.0, 0.0, 0.0),
        v(10.0, 0.0, 0.0),
        0.5,
    );
    assert!(!tr.hit, "{tr:?}");
}

#[test]
fn a_turned_mover_collides_where_it_now_is_with_a_turned_normal() {
    let bsp = room();
    let mut movers = [door()];
    // A quarter turn about +Z through the slab's corner at (5, -2): it now lies
    // along y from -2 to -1, x from 1 to 5.
    let pose = Pose::turned(v(0.0, 0.0, 1.0), FRAC_PI_2, vd(5.0, -2.0, 0.0));
    movers[0].set_pose(&pose);
    let solids = Solids::new(&bsp, &movers);
    let through = sweep_sphere(solids, v(0.0, 0.0, 0.0), v(10.0, 0.0, 0.0), 0.5);
    assert!(!through.hit, "the old place is empty: {through:?}");
    let down = sweep_sphere(solids, v(3.0, 3.0, 0.0), v(3.0, -8.0, 0.0), 0.5);
    assert_eq!(down.mover, Some(0));
    assert!((down.normal - v(0.0, 1.0, 0.0)).length() < 1e-5, "{down:?}");
    assert!((down.end.y - -0.5).abs() < 0.01, "{down:?}");
}

#[test]
fn a_ray_with_no_radius_hits_a_mover() {
    let bsp = room();
    let movers = [door()];
    let tr = sweep_sphere(
        Solids::new(&bsp, &movers),
        v(0.0, 0.0, 0.0),
        v(10.0, 0.0, 0.0),
        0.0,
    );
    assert_eq!(tr.mover, Some(0));
}

#[test]
fn a_centre_inside_a_mover_can_leave_instead_of_freezing() {
    let bsp = room();
    let movers = [door()];
    let solids = Solids::new(&bsp, &movers);
    let tr = sweep_sphere(solids, v(5.5, 0.0, 0.0), v(-3.0, 0.0, 0.0), 0.5);
    assert!(!tr.hit && !tr.start_solid, "{tr:?}");
}

#[test]
fn slides_run_along_a_mover() {
    let bsp = room();
    let movers = [door()];
    let solids = Solids::new(&bsp, &movers);
    let r = slide_move(solids, v(4.0, 0.0, 0.0), v(10.0, 10.0, 0.0), 0.05, 0.5);
    assert!(r.pos.x <= 4.5 && r.pos.y > 0.4, "{r:?}");
    assert_eq!(r.contacts, 1);
}

fn body_at(p: Vec3, r: f32) -> Body {
    Body::new(p, r)
}

#[test]
fn a_mover_carries_a_body_ahead_of_it() {
    let bsp = room();
    let mut movers = [door()];
    let mut body = body_at(v(4.0, 0.0, 0.0), 0.5);
    let pose = Pose::translated(vd(-3.0, 0.0, 0.0));
    assert_eq!(
        push_mover(&bsp, &mut movers, 0, &pose, &mut [&mut body]),
        Push::Moved
    );
    // The door's face went from x = 5 to x = 2; the body stays half a unit off.
    assert!((body.pos.x - 1.5).abs() < 0.01, "{body:?}");
    assert_eq!(
        body.vel,
        Vec3::ZERO,
        "pushing changes position, not velocity"
    );
}

#[test]
fn a_very_fast_mover_cannot_jump_over_a_body() {
    let bsp = room();
    let mut movers = [door()];
    let mut body = body_at(v(0.0, 0.0, 0.0), 0.1);
    let pose = Pose::translated(vd(-20.0, 0.0, 0.0));
    let result = push_mover(&bsp, &mut movers, 0, &pose, &mut [&mut body]);
    assert_eq!(result, Push::Moved);
    assert!(body.pos.x < -15.0 + 0.01, "left behind: {body:?}");
}

#[test]
fn a_body_against_a_wall_blocks_and_nothing_moves() {
    // The room ends at x = -20; the door tries to close the gap to nothing.
    let bsp = room();
    let mut movers = [door()];
    let mut near = body_at(v(4.0, 0.0, 0.0), 0.5);
    let mut far = body_at(v(4.0, 1.0, 1.0), 0.5);
    let before = (near.pos, far.pos, movers[0].clone());
    let pose = Pose::translated(vd(-25.0, 0.0, 0.0));
    let result = push_mover(&bsp, &mut movers, 0, &pose, &mut [&mut near, &mut far]);
    assert_eq!(result, Push::Blocked(0));
    assert_eq!((near.pos, far.pos, movers[0].clone()), before);
}

#[test]
fn a_body_pushed_at_an_angle_slides_along_the_wall_instead_of_blocking() {
    let walls = compile(&[room_brush()]).unwrap();
    // A slab tilted 45 degrees, so its underside faces down and toward -x,
    // comes down on a body resting on the floor at z = -20.
    let slab = Brush::cuboid(vd(-4.0, -4.0, 0.0), vd(4.0, 4.0, 1.0), Op::Add, 0).unwrap();
    let shape = Arc::new(MoverShape::from_brushes(std::iter::once(&slab)));
    let mut movers = [MoverSolid::new(shape)];
    let tilt = Pose::turned(v(0.0, 1.0, 0.0), FRAC_PI_2 / 2.0, vd(0.0, 0.0, 0.0));
    let at = |z| Pose {
        offset: vd(0.0, 0.0, z),
        ..tilt
    };
    movers[0].set_pose(&at(-18.6));
    let mut body = body_at(v(0.0, 0.0, -19.5), 0.5);
    let result = push_mover(&walls, &mut movers, 0, &at(-19.6), &mut [&mut body]);
    assert_eq!(result, Push::Moved, "{body:?}");
    assert!(
        body.pos.x < -0.7,
        "it should have slid out sideways: {body:?}"
    );
    assert!(
        body.pos.z >= -19.51,
        "it must stay above the floor: {body:?}"
    );
}

#[test]
fn a_body_not_in_the_way_is_left_alone() {
    let bsp = room();
    let mut movers = [door()];
    let mut body = body_at(v(0.0, 10.0, 0.0), 0.5);
    let pose = Pose::translated(vd(-8.0, 0.0, 0.0));
    let start = body.pos;
    assert_eq!(
        push_mover(&bsp, &mut movers, 0, &pose, &mut [&mut body]),
        Push::Moved
    );
    assert_eq!(body.pos, start);
}

#[test]
fn a_body_with_its_centre_inside_does_not_block() {
    let bsp = room();
    let mut movers = [door()];
    let mut body = body_at(v(5.5, 0.0, 0.0), 0.5);
    let pose = Pose::translated(vd(-3.0, 0.0, 0.0));
    assert_eq!(
        push_mover(&bsp, &mut movers, 0, &pose, &mut [&mut body]),
        Push::Moved
    );
}

#[test]
fn a_hinge_stays_on_its_hinge_all_the_way_round() {
    let pivot = vd(3.0, 4.0, 5.0);
    let open = Pose::turned(v(0.0, 0.0, 1.0), 2.0, pivot);
    for k in 0..=10 {
        let s = k as f32 / 10.0;
        let pose = Pose::between(&Pose::IDENTITY, &open, s);
        let frame = crate::mover::Frame::of(&pose);
        assert!((frame.to_world(pivot) - pivot).length() < 1e-5, "s {s}");
        // Half way round is half the angle.
        let angle = Pose::IDENTITY.angle_to(&pose);
        assert!((angle - 2.0 * s).abs() < 1e-3, "s {s} angle {angle}");
    }
}

#[test]
fn between_is_exact_at_both_ends_and_continuous_across_pivots() {
    let a = Pose::turned(v(1.0, 0.0, 0.0), 1.0, vd(1.0, 2.0, 3.0));
    let b = Pose::turned(v(0.0, 1.0, 0.0), 0.5, vd(-4.0, 0.0, 9.0));
    assert_eq!(Pose::between(&a, &b, 0.0), a);
    assert_eq!(Pose::between(&a, &b, 1.0), b);
    let p = vd(5.0, 5.0, 5.0);
    let at = |pose: &Pose| crate::mover::Frame::of(pose).to_world(p);
    let just_after = Pose::between(&a, &b, 1e-4);
    assert!((at(&just_after) - at(&a)).length() < 0.01);
    let just_before = Pose::between(&a, &b, 1.0 - 1e-4);
    assert!((at(&just_before) - at(&b)).length() < 0.01);
}

#[test]
fn frames_invert_exactly() {
    let pose = Pose {
        offset: vd(1.0, -2.0, 3.0),
        ..Pose::turned(v(1.0, 2.0, 3.0), 1.3, vd(7.0, 8.0, 9.0))
    };
    let frame = crate::mover::Frame::of(&pose);
    let p = vd(-3.0, 4.5, 100.0);
    assert!((frame.to_local(frame.to_world(p)) - p).length() < 1e-9);
}

#[test]
fn a_mover_that_does_not_change_pose_touches_nothing() {
    let bsp = room();
    let mut movers = [door()];
    let mut body = body_at(v(5.5, 0.0, 0.0), 0.5);
    let result = push_mover(&bsp, &mut movers, 0, &Pose::IDENTITY, &mut [&mut body]);
    assert_eq!(result, Push::Moved);
    assert_eq!(body.pos, v(5.5, 0.0, 0.0));
}

#[test]
fn a_blocked_step_undoes_the_pushes_before_it() {
    let bsp = room();
    // The door ends at y = 2, so this body only clips its edge and is shoved
    // aside, while the second is pressed against the far wall of the room.
    let start = v(4.0, 2.2, 0.0);
    let pose = Pose::translated(vd(-25.0, 0.0, 0.0));
    let mut alone = body_at(start, 0.5);
    let mut movers = [door()];
    let result = push_mover(&bsp, &mut movers, 0, &pose, &mut [&mut alone]);
    assert_eq!(result, Push::Moved);
    assert_ne!(alone.pos, start, "the set-up needs this body to get pushed");

    let mut movers = [door()];
    let mut clipped = body_at(start, 0.5);
    let mut pressed = body_at(v(4.0, 0.0, 0.0), 0.5);
    let result = push_mover(
        &bsp,
        &mut movers,
        0,
        &pose,
        &mut [&mut clipped, &mut pressed],
    );
    assert_eq!(result, Push::Blocked(1));
    assert_eq!(clipped.pos, start, "its push must be undone");
}

#[test]
fn a_fast_rotor_still_sweeps_a_body_ahead_of_it() {
    let bsp = room();
    // A blade along +x from the origin, turning towards +y, a body in its way.
    let blade = Brush::cuboid(vd(1.0, -0.25, -1.0), vd(5.0, 0.25, 1.0), Op::Add, 0).unwrap();
    let shape = Arc::new(MoverShape::from_brushes(std::iter::once(&blade)));
    for steps in [4, 16, 64, 256] {
        let mut movers = [MoverSolid::new(shape.clone())];
        let mut body = body_at(v(3.0, 2.0, 0.0), 0.5);
        for k in 1..=steps {
            let angle = FRAC_PI_2 * k as f32 / steps as f32;
            let pose = Pose::turned(v(0.0, 0.0, 1.0), angle, vd(0.0, 0.0, 0.0));
            assert_eq!(
                push_mover(&bsp, &mut movers, 0, &pose, &mut [&mut body]),
                Push::Moved
            );
        }
        // The blade now lies along +y. The body was shoved round ahead of it.
        assert!(body.pos.x < -0.7, "{steps} steps: left behind at {body:?}");
    }
}
