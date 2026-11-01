use crate::slide::{MAX_BUMPS, slide_move};
use crate::test_util::*;
use mscent_math::Vec3;

const R: f32 = 1.0;

fn dist_to_x_wall(p: Vec3) -> f32 {
    20.0 - p.x
}

#[test]
fn free_move_is_untouched() {
    let r = slide_move(&room(), v(0.0, 0.0, 0.0), v(3.0, 0.0, 0.0), 1.0, R);
    assert_eq!(r.pos, v(3.0, 0.0, 0.0));
    assert_eq!(r.vel, v(3.0, 0.0, 0.0));
    assert_eq!(r.contacts, 0);
}

#[test]
fn glancing_wall_hit_keeps_tangential_motion() {
    // 5 degrees into the +x wall at speed 10 for one second.
    let a = 5.0_f32.to_radians();
    let vel = v(10.0 * a.sin(), 10.0 * a.cos(), 0.0);
    let r = slide_move(&room(), v(18.5, -15.0, 0.0), vel, 1.0, R);
    assert_eq!(r.contacts, 1);
    assert!(dist_to_x_wall(r.pos) >= R, "{:?}", r.pos);
    assert!(dist_to_x_wall(r.pos) < R + 0.01);
    assert_eq!(r.vel.x, 0.0);
    assert!((r.vel.y - vel.y).abs() < 1e-3);
    // Sliding preserves the along-wall speed, so y advances by the full 10 cos(a).
    assert!((r.pos.y - (-15.0 + vel.y)).abs() < 0.05, "{:?}", r.pos);
}

#[test]
fn head_on_wall_stops_dead() {
    let r = slide_move(&room(), v(10.0, 0.0, 0.0), v(50.0, 0.0, 0.0), 1.0, R);
    assert_eq!(r.vel, Vec3::ZERO);
    assert!(dist_to_x_wall(r.pos) >= R);
}

#[test]
fn slope_deflects_velocity_along_the_surface() {
    let s = std::f32::consts::FRAC_1_SQRT_2;
    let r = slide_move(
        &room_with_ramp(),
        v(5.0, 0.0, 10.0),
        v(0.0, 0.0, -10.0),
        1.0,
        R,
    );
    assert_eq!(r.contacts, 1);
    let n = r.touched()[0];
    assert!((n.x + s).abs() < 1e-3 && (n.z - s).abs() < 1e-3);
    // Velocity is now tangent to the slope: down and toward -x.
    assert!(r.vel.dot(n).abs() < 1e-3);
    assert!(r.vel.x < -4.0 && r.vel.z < -4.0);
    // And the sphere rides the surface, a skin above one radius.
    let h = n.dot(r.pos);
    assert!((R..R + 0.01).contains(&h), "{h}");
}

#[test]
fn two_plane_corner_stops_without_creeping() {
    let bsp = room();
    let mut pos = v(15.0, 12.0, 0.0);
    let mut history = Vec::new();
    for _ in 0..200 {
        let r = slide_move(&bsp, pos, v(10.0, 10.0, 0.0), 1.0 / 120.0, R);
        pos = r.pos;
        history.push(pos);
    }
    assert!(pos.x <= 19.0 && pos.y <= 19.0);
    assert!(pos.x > 18.99 && pos.y > 18.99, "{pos:?}");
    // Settled: the last 50 steps do not move at all.
    let last = history[199];
    assert!(history[150..].iter().all(|p| (*p - last).length() < 1e-5));
}

#[test]
fn two_plane_crease_slides_along_the_edge() {
    // Pushing into the vertical corner while also rising: the crease is z.
    let r = slide_move(&room(), v(15.0, 12.0, 0.0), v(10.0, 10.0, 6.0), 1.0, R);
    assert!(r.pos.x > 18.99 && r.pos.y > 18.99);
    assert!(r.pos.z > 1.0, "should have climbed the crease: {:?}", r.pos);
    assert!(r.vel.x.abs() < 1e-4 && r.vel.y.abs() < 1e-4);
    assert!(r.vel.z > 5.9);
}

#[test]
fn three_plane_corner_pins_the_sphere() {
    let bsp = room();
    let mut pos = v(15.0, 15.0, 15.0);
    let mut last = pos;
    for i in 0..150 {
        let r = slide_move(&bsp, pos, v(8.0, 9.0, 10.0), 1.0 / 60.0, R);
        assert!(r.contacts <= MAX_BUMPS);
        pos = r.pos;
        if i == 100 {
            last = pos;
        }
    }
    assert!((pos - last).length() < 1e-5, "{pos:?} vs {last:?}");
    assert!(pos.x > 18.99 && pos.y > 18.99 && pos.z > 18.99);
    assert!(pos.x <= 19.0 && pos.y <= 19.0 && pos.z <= 19.0);
}

#[test]
fn rounds_a_block_edge_instead_of_catching() {
    let bsp = room_with_block();
    // Pushing +x just beside the block's vertical edge: slides off it.
    let r = slide_move(&bsp, v(-3.0, -0.6, 2.0), v(10.0, 0.0, 0.0), 1.0, R);
    assert!(r.contacts >= 1);
    assert!(r.pos.x > -0.8, "made progress around the edge: {:?}", r.pos);
    assert!(r.pos.y < -0.6, "deflected away from the block: {:?}", r.pos);
    let edge_dist = (r.pos.x * r.pos.x + r.pos.y * r.pos.y).sqrt();
    assert!(edge_dist >= R, "{edge_dist}");
}

#[test]
fn bad_input_does_not_panic_or_move() {
    let r = slide_move(&room(), v(f32::NAN, 0.0, 0.0), v(1.0, 0.0, 0.0), 1.0, R);
    assert_eq!(r.vel, Vec3::ZERO);
    let r = slide_move(
        &room(),
        v(0.0, 0.0, 0.0),
        v(1.0, f32::INFINITY, 0.0),
        1.0,
        R,
    );
    assert_eq!(r.vel, Vec3::ZERO);
    let inside_rock = slide_move(&room(), v(30.0, 0.0, 0.0), v(1.0, 0.0, 0.0), 1.0, R);
    assert_eq!(inside_rock.pos, v(30.0, 0.0, 0.0));
}
