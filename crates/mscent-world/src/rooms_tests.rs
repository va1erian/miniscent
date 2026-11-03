use super::*;
use crate::{Brush, Op, compile};
use mscent_math::{Vec3, Vec3d};

fn cuboid(a: [f64; 3], b: [f64; 3], op: Op, order: u32) -> Brush {
    let v = |p: [f64; 3]| Vec3d::new(p[0], p[1], p[2]);
    Brush::cuboid(v(a), v(b), op, order).expect("a brush")
}

fn rooms_of(brushes: &[Brush]) -> (Bsp, Rooms) {
    let bsp = compile(brushes).expect("a tree");
    let portals = Portals::build(&bsp);
    let rooms = Rooms::build(&bsp, &portals);
    (bsp, rooms)
}

fn room_at(bsp: &Bsp, rooms: &Rooms, p: [f32; 3]) -> u32 {
    let leaf = bsp.leaf_at_f32(Vec3::new(p[0], p[1], p[2]));
    rooms.room_of(leaf).expect("a point in a room")
}

/// Two 20-unit rooms with a doorway between, `door` units long.
fn two_rooms(door: f64) -> Vec<Brush> {
    let b = 20.0 + door;
    vec![
        cuboid([-20.0, -10.0, -10.0], [0.0, 10.0, 10.0], Op::Subtract, 0),
        cuboid([0.0, -2.0, -2.0], [door, 2.0, 2.0], Op::Subtract, 1),
        cuboid([door, -10.0, -10.0], [b, 10.0, 10.0], Op::Subtract, 2),
    ]
}

#[test]
fn a_pillar_does_not_split_a_room() {
    let (bsp, rooms) = rooms_of(&[
        cuboid([-20.0; 3], [20.0; 3], Op::Subtract, 0),
        cuboid([-3.0, -3.0, -20.0], [3.0, 3.0, 20.0], Op::Add, 1),
    ]);
    let empty = bsp
        .leaves()
        .iter()
        .filter(|l| l.kind == LeafKind::Empty)
        .count();
    assert!(
        empty > 1,
        "the pillar's planes cut the room into {empty} leaves"
    );
    assert_eq!(rooms.count(), 1);
}

#[test]
fn a_doorway_keeps_two_rooms_apart_and_a_light_in_it_reaches_both() {
    let (bsp, rooms) = rooms_of(&two_rooms(2.0));
    let (a, b) = (
        room_at(&bsp, &rooms, [-10.0; 3]),
        room_at(&bsp, &rooms, [11.0, 0.0, 0.0]),
    );
    assert_ne!(a, b);
    let door = room_at(&bsp, &rooms, [1.0, 0.0, 0.0]);
    for room in [a, b] {
        assert!(
            rooms.reaches(door, room, 1.0),
            "the doorway is at the rooms' walls"
        );
    }
}

#[test]
fn rooms_with_no_portal_between_never_reach_each_other() {
    let (bsp, rooms) = rooms_of(&[
        cuboid([-20.0, -10.0, -10.0], [0.0, 10.0, 10.0], Op::Subtract, 0),
        cuboid([2.0, -10.0, -10.0], [22.0, 10.0, 10.0], Op::Subtract, 1),
    ]);
    let (a, b) = (
        room_at(&bsp, &rooms, [-5.0, 0.0, 0.0]),
        room_at(&bsp, &rooms, [5.0, 0.0, 0.0]),
    );
    assert_eq!(rooms.step(a, b), Some(NEVER));
    assert!(!rooms.reaches(a, b, MAX_LIGHT_REACH));
    assert!(rooms.reaches(a, a, 0.5));
}

#[test]
fn a_chain_costs_its_worst_portal() {
    // Rooms 20 across, a corridor of 100 between: the far doorway is 100 from
    // the first room's box, so a light in it cannot get there by any radius.
    let (bsp, rooms) = rooms_of(&two_rooms(100.0));
    let (a, b) = (
        room_at(&bsp, &rooms, [-10.0; 3]),
        room_at(&bsp, &rooms, [110.0, 0.0, 0.0]),
    );
    assert!(!rooms.reaches(a, b, MAX_LIGHT_REACH));
    let hall = room_at(&bsp, &rooms, [50.0, 0.0, 0.0]);
    assert!(
        rooms.reaches(a, hall, 1.0),
        "its doorway is on the box of a"
    );
    assert!(!rooms.reaches(a, hall, 0.0), "no light has no radius");
}

#[test]
fn every_room_is_at_zero_from_itself() {
    let (_, rooms) = rooms_of(&two_rooms(10.0));
    assert!((0..rooms.count() as u32).all(|r| rooms.step(r, r) == Some(0)));
    assert_eq!(rooms.table_bytes(), rooms.count() * rooms.count() * 2);
}

#[test]
fn hostile_parts_are_refused() {
    let ok = || Rooms::from_parts(vec![0, NO_ROOM], 1, vec![0]);
    assert!(ok().is_ok());
    let many = MAX_ROOMS + 1;
    assert_eq!(
        Rooms::from_parts(vec![], many, vec![]),
        Err(RoomError::TooMany)
    );
    assert_eq!(
        Rooms::from_parts(vec![], usize::MAX, vec![]),
        Err(RoomError::TooMany)
    );
    assert_eq!(
        Rooms::from_parts(vec![0], 1, vec![]),
        Err(RoomError::BadTable)
    );
    assert_eq!(
        Rooms::from_parts(vec![0], 2, vec![0, 0, 0]),
        Err(RoomError::BadTable)
    );
    assert_eq!(
        Rooms::from_parts(vec![1], 1, vec![0]),
        Err(RoomError::BadRoom)
    );
    assert_eq!(
        Rooms::from_parts(vec![0], 1, vec![5]),
        Err(RoomError::BadDiagonal)
    );
    assert_eq!(Rooms::from_parts(vec![], 0, vec![]), Ok(Rooms::default()));
}

#[test]
fn a_forged_table_cannot_make_a_lookup_leave_the_table() {
    let rooms = Rooms::from_parts(vec![0, 1], 2, vec![0, 3, 7, 0]).expect("a table");
    assert_eq!(rooms.step(0, 1), Some(3));
    assert_eq!(rooms.step(1, 0), Some(7));
    assert_eq!(rooms.step(2, 0), None);
    assert_eq!(rooms.step(0, 2), None);
    assert_eq!(rooms.step(u32::MAX, u32::MAX), None);
    assert!(!rooms.reaches(5, 0, 64.0));
    assert_eq!(rooms.room_of(LeafId(2)), None);
    assert_eq!(rooms.row(u32::MAX), None);
}

/// A grouping of `count` rooms and no leaves, to be told apart by its count.
fn counted(count: usize) -> Parts {
    Parts {
        of_leaf: Vec::new(),
        count,
        boxes: Vec::new(),
    }
}

#[test]
fn the_grouping_kept_is_the_first_that_fits() {
    let never = || -> Parts { panic!("not asked for") };
    assert_eq!(fit(4, counted(4), never, never).count, 4);
    assert_eq!(fit(4, counted(5), || counted(3), never).count, 3);
    assert_eq!(fit(4, counted(5), || counted(4), never).count, 4);
    assert_eq!(fit(4, counted(5), || counted(5), || counted(1)).count, 1);
    assert_eq!(fit(0, counted(0), never, never).count, 0);
}

fn built_within(brushes: &[Brush], most: usize) -> (Bsp, Rooms) {
    let bsp = compile(brushes).expect("a tree");
    let portals = Portals::build(&bsp);
    let rooms = Rooms::build_within(&bsp, &portals, most);
    (bsp, rooms)
}

#[test]
fn more_rooms_than_allowed_become_the_islands_of_portals() {
    // Two rooms and the doorway between: three rooms, and one island.
    let brushes = two_rooms(2.0);
    assert_eq!(built_within(&brushes, 3).1.count(), 3);
    let (bsp, rooms) = built_within(&brushes, 2);
    assert_eq!(rooms.count(), 1);
    let (a, b) = (
        room_at(&bsp, &rooms, [-10.0; 3]),
        room_at(&bsp, &rooms, [11.0, 0.0, 0.0]),
    );
    assert_eq!(a, b);
    assert!(rooms.reaches(a, b, 1.0));
}

#[test]
fn more_islands_than_allowed_become_one_room_that_every_light_reaches_by_radius() {
    // Two rooms with no portal between: two islands, and one allowed.
    let brushes = [
        cuboid([-20.0, -10.0, -10.0], [0.0, 10.0, 10.0], Op::Subtract, 0),
        cuboid([2.0, -10.0, -10.0], [22.0, 10.0, 10.0], Op::Subtract, 1),
    ];
    assert_eq!(built_within(&brushes, 2).1.count(), 2);
    let (bsp, rooms) = built_within(&brushes, 1);
    assert_eq!(rooms.count(), 1, "not zero, which would light nothing");
    let (a, b) = (
        room_at(&bsp, &rooms, [-5.0, 0.0, 0.0]),
        room_at(&bsp, &rooms, [5.0, 0.0, 0.0]),
    );
    assert_eq!((a, b), (0, 0));
    assert!(rooms.reaches(a, b, 0.5), "the radius is all that limits it");
    assert!(!rooms.reaches(a, b, 0.0), "no radius reaches nothing");
    assert_eq!(rooms.table_bytes(), 2);
    assert!(rooms.table_bytes() <= MAX_ROOMS * MAX_ROOMS * 2);
}
