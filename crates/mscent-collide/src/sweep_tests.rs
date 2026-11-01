use crate::sweep::sweep_sphere;
use crate::test_util::*;
use crate::tuning::SKIN;
use mscent_math::rng::Rng;

const TOL: f32 = 2.0e-3;
const SKIN32: f32 = SKIN as f32;

fn near(a: f32, b: f32) -> bool {
    (a - b).abs() < TOL
}

#[test]
fn free_move_in_open_room_does_not_hit() {
    let t = sweep_sphere(&room(), v(0.0, 0.0, 0.0), v(10.0, 5.0, -3.0), 1.0);
    assert!(!t.hit && !t.start_solid);
    assert_eq!(t.fraction, 1.0);
    assert_eq!(t.end, v(10.0, 5.0, -3.0));
}

#[test]
fn wall_stops_sphere_one_radius_away() {
    let t = sweep_sphere(&room(), v(0.0, 0.0, 0.0), v(30.0, 0.0, 0.0), 1.0);
    assert!(t.hit);
    assert!(near(t.fraction, 19.0 / 30.0), "{}", t.fraction);
    assert!(near(t.end.x, 19.0 - SKIN32));
    assert_eq!(t.normal, v(-1.0, 0.0, 0.0));
}

#[test]
fn radius_changes_stop_distance() {
    let t = sweep_sphere(&room(), v(0.0, 0.0, 0.0), v(0.0, 0.0, -30.0), 2.5);
    assert!(near(t.end.z, -17.5 + SKIN32));
    assert_eq!(t.normal, v(0.0, 0.0, 1.0));
}

#[test]
fn slope_contact_is_at_pushed_out_plane() {
    let t = sweep_sphere(&room_with_ramp(), v(5.0, 0.0, 10.0), v(5.0, 0.0, -5.0), 1.0);
    let s = std::f32::consts::FRAC_1_SQRT_2;
    assert!(t.hit);
    assert!(near(t.end.z, 5.0 + 2.0_f32.sqrt() + SKIN32 * s));
    assert!(near(t.normal.x, -s) && near(t.normal.z, s));
}

#[test]
fn grazing_parallel_to_wall_is_not_blocked() {
    // Resting exactly one radius off the wall and moving along it.
    let t = sweep_sphere(&room(), v(19.0, -5.0, 0.0), v(19.0, 15.0, 0.0), 1.0);
    assert!(!t.hit, "{t:?}");
}

#[test]
fn glancing_shallow_angle_hits_far_along_the_wall() {
    // 1 unit of approach over 30 along the wall: contact at fraction 0.5.
    let t = sweep_sphere(&room(), v(18.0, -15.0, 0.0), v(20.0, 15.0, 0.0), 1.0);
    assert!(t.hit);
    assert!(near(t.fraction, 0.5), "{}", t.fraction);
    assert!(near(t.end.x, 19.0 - SKIN32));
}

#[test]
fn leaving_a_wall_is_free_and_embedded_pushes_out() {
    let away = sweep_sphere(&room(), v(19.0, 0.0, 0.0), v(10.0, 0.0, 0.0), 1.0);
    assert!(!away.hit);
    // Centre 0.5 from the wall with radius 1: moving in hits at once.
    let into = sweep_sphere(&room(), v(19.5, 0.0, 0.0), v(19.9, 0.0, 0.0), 1.0);
    assert!(into.hit && into.fraction == 0.0);
}

#[test]
fn start_inside_rock_or_bad_input_is_blocked() {
    let t = sweep_sphere(&room(), v(30.0, 0.0, 0.0), v(0.0, 0.0, 0.0), 1.0);
    assert!(t.start_solid && t.fraction == 0.0);
    let nan = sweep_sphere(&room(), v(f32::NAN, 0.0, 0.0), v(0.0, 0.0, 0.0), 1.0);
    assert!(nan.start_solid);
    let neg = sweep_sphere(&room(), v(0.0, 0.0, 0.0), v(1.0, 0.0, 0.0), -1.0);
    assert!(neg.start_solid);
}

#[test]
fn face_of_block_is_hit_at_pushed_out_plane() {
    let t = sweep_sphere(
        &room_with_block(),
        v(-10.0, 2.0, 2.0),
        v(10.0, 2.0, 2.0),
        1.0,
    );
    assert!(near(t.end.x, -1.0 - SKIN32));
    assert_eq!(t.normal, v(-1.0, 0.0, 0.0));
}

#[test]
fn convex_edge_stops_at_rounded_distance_not_square() {
    // Block edge along z at x = y = 0. The centre line passes 0.6 beside it,
    // so contact is at |c - edge| = 1: x = -sqrt(1 - 0.36) = -0.8. A square
    // corner (the plane pushed out and extended) would stop at -1.
    let t = sweep_sphere(
        &room_with_block(),
        v(-5.0, -0.6, 2.0),
        v(5.0, -0.6, 2.0),
        1.0,
    );
    assert!(t.hit);
    let expect_x = -0.8 - 0.8 * SKIN32;
    assert!(near(t.end.x, expect_x), "end {:?}", t.end);
    assert!(
        t.end.x > -0.95,
        "must not stop at the square-corner distance"
    );
    assert!(near(t.normal.x, -0.8) && near(t.normal.y, -0.6) && near(t.normal.z, 0.0));
}

#[test]
fn convex_vertex_stops_at_rounded_distance() {
    // Corner (0,0,0): centre line 0.5 off in y and z, contact at x = -sqrt(0.5).
    let t = sweep_sphere(
        &room_with_block(),
        v(-5.0, -0.5, -0.5),
        v(5.0, -0.5, -0.5),
        1.0,
    );
    let r = 0.5_f32.sqrt();
    assert!(t.hit);
    assert!(near(t.end.x, -r - r * SKIN32), "end {:?}", t.end);
    assert!(near(t.normal.x, -r) && near(t.normal.y, -0.5) && near(t.normal.z, -0.5));
}

#[test]
fn sphere_past_the_edge_rounds_it_without_catching() {
    // 1.2 beside the edge (more than the radius): passes the block entirely.
    let t = sweep_sphere(
        &room_with_block(),
        v(-5.0, -1.2, 2.0),
        v(10.0, -1.2, 2.0),
        1.0,
    );
    assert!(!t.hit, "{t:?}");
}

#[test]
fn random_sweeps_never_end_in_or_cross_solid() {
    let bsp = room_with_block();
    let mut rng = Rng::new(0xC0FFEE);
    let r = 0.75;
    let mut tested = 0;
    for _ in 0..3000 {
        let mut p = || {
            v(
                rng.range_f32(-19.0, 19.0),
                rng.range_f32(-19.0, 19.0),
                rng.range_f32(-19.0, 19.0),
            )
        };
        let (from, to) = (p(), p());
        if block_room_clearance(from) < r {
            continue;
        }
        tested += 1;
        let t = sweep_sphere(&bsp, from, to, r);
        assert!(!t.start_solid);
        for i in 0..=40 {
            let s = t.fraction * (i as f32 / 40.0);
            let at = if i == 40 {
                t.end
            } else {
                from + (to - from) * s
            };
            let clear = block_room_clearance(at);
            assert!(
                clear >= r - 1.0e-4,
                "from {from:?} to {to:?} at {s}: {clear}"
            );
        }
    }
    assert!(tested > 1000);
}

#[test]
fn random_sweeps_on_the_ramp_keep_centres_and_clearance_out_of_rock() {
    let bsp = room_with_ramp();
    let s = std::f32::consts::FRAC_1_SQRT_2;
    let mut rng = Rng::new(77);
    let r = 0.5;
    for _ in 0..3000 {
        let mut p = || {
            v(
                rng.range_f32(-9.0, 9.0),
                rng.range_f32(-9.0, 9.0),
                rng.range_f32(-9.0, 15.0),
            )
        };
        let (from, to) = (p(), p());
        // Above the slope plane by more than the radius, so the start is legal.
        if (from.z - from.x) * s < r {
            continue;
        }
        let t = sweep_sphere(&bsp, from, to, r);
        let end_height = (t.end.z - t.end.x) * s;
        assert!(end_height >= r - 1.0e-4, "{from:?} -> {to:?}: {t:?}");
    }
}
