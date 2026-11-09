//! The sky's pieces alone: a window's crossing, the map, and hostile rooms.

use super::*;
use mscent_math::Vec3;

/// The square window x, z in -1..1 on the plane y = 0, looking down -y.
fn window() -> Window {
    let v = |x: f64, z: f64| Vec3d::new(x, 0.0, z);
    let plane = DPlane::new(Vec3d::new(0.0, -1.0, 0.0), 0.0);
    // Counter-clockwise seen from -y.
    Window::new(
        plane,
        vec![v(-1.0, -1.0), v(1.0, -1.0), v(1.0, 1.0), v(-1.0, 1.0)],
    )
    .unwrap()
}

fn map() -> RoomMap {
    RoomMap {
        pos: Vec3d::new(1000.0, 0.0, 0.0),
        centre: Vec3d::ZERO,
        scale: 0.25,
    }
}

fn lamp(position: Vec3) -> PointLight {
    PointLight {
        position,
        color: Vec3::new(1.0, 1.0, 1.0),
        radius: 10.0,
        cone: None,
    }
}

fn room() -> SkyRoom {
    SkyRoom::new(map(), vec![window()], &[lamp(Vec3::new(1000.0, 5.0, 0.0))]).unwrap()
}

#[test]
fn a_segment_crosses_a_window_only_through_it_and_from_its_front() {
    let w = window();
    let (front, back) = (Vec3d::new(0.0, -4.0, 0.0), Vec3d::new(0.0, 4.0, 0.0));
    assert_eq!(w.crossing(front, back), Some(0.5));
    assert_eq!(w.crossing(back, front), None, "from behind");
    let beside = Vec3d::new(3.0, 4.0, 0.0);
    assert_eq!(
        w.crossing(Vec3d::new(3.0, -4.0, 0.0), beside),
        None,
        "past its edge"
    );
    assert_eq!(
        w.crossing(front, Vec3d::new(0.0, -1.0, 0.0)),
        None,
        "short of it"
    );
}

#[test]
fn a_room_lamp_is_carried_out_through_the_inverse_map() {
    let r = room();
    let out = r.lamps()[0];
    // (1000, 5, 0) in the room is (0, 20, 0) in the play space, 4 times as wide.
    assert_eq!(out.position, Vec3::new(0.0, 20.0, 0.0));
    assert_eq!(out.radius, 40.0);
    let back = map().in_room(map().in_play(Vec3d::new(1003.0, -2.0, 7.0)));
    assert!((back - Vec3d::new(1003.0, -2.0, 7.0)).length() < 1e-9);
}

#[test]
fn a_ray_sees_the_lamp_only_through_a_window_with_both_parts_clear() {
    let r = room();
    let (from, to) = (Vec3d::new(0.0, -4.0, 0.0), Vec3d::new(0.0, 20.0, 0.0));
    assert!(r.sees(from, to, |_, _| true));
    assert!(!r.sees(from, to, |_, _| false));
    // The room part runs from the window's image to the lamp, in the room.
    let room_part = |a: Vec3d, b: Vec3d| a.x > 900.0 && b.x > 900.0;
    let play_part = |a: Vec3d, b: Vec3d| a.x < 100.0 && b.x < 100.0;
    assert!(!r.sees(from, to, room_part));
    assert!(!r.sees(from, to, play_part));
    assert!(r.sees(from, to, |a, b| room_part(a, b) || play_part(a, b)));
    // Past the window's edge there is no way through.
    assert!(!r.sees(
        Vec3d::new(5.0, -4.0, 0.0),
        Vec3d::new(5.0, 20.0, 0.0),
        |_, _| true
    ));
}

#[test]
fn hostile_rooms_are_refused_and_bad_lamps_dropped() {
    let lamps = [lamp(Vec3::new(1000.0, 5.0, 0.0))];
    for scale in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        assert!(SkyRoom::new(RoomMap { scale, ..map() }, vec![window()], &lamps).is_none());
    }
    let far = RoomMap {
        pos: Vec3d::new(f64::NAN, 0.0, 0.0),
        ..map()
    };
    assert!(SkyRoom::new(far, vec![window()], &lamps).is_none());
    assert!(
        SkyRoom::new(map(), Vec::new(), &lamps).is_none(),
        "no window"
    );
    let tiny = RoomMap {
        scale: 1e-300,
        ..map()
    };
    assert!(
        SkyRoom::new(tiny, vec![window()], &lamps).is_none(),
        "lamps out of range"
    );
    let plane = DPlane::new(Vec3d::new(0.0, -1.0, 0.0), f64::NAN);
    assert!(Window::new(plane, vec![Vec3d::ZERO; 3]).is_none());
    assert!(
        Window::new(
            DPlane::new(Vec3d::new(0.0, 0.0, 1.0), 0.0),
            vec![Vec3d::ZERO; 2]
        )
        .is_none()
    );
}

#[test]
fn no_sky_keeps_the_key_and_a_sky_changes_it() {
    assert_eq!(Sky::default().shape_key(), 0);
    let one = Sky::new(vec![room()]).shape_key();
    assert_ne!(one, 0);
    let moved = SkyRoom::new(map(), vec![window()], &[lamp(Vec3::new(1000.0, 6.0, 0.0))]);
    assert_ne!(Sky::new(vec![moved.unwrap()]).shape_key(), one);
}
