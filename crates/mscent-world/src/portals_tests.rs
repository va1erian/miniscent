use crate::brush::{Brush, Op};
use crate::bsp::{Bsp, LeafId, LeafKind};
use crate::csg::compile;
use crate::portals::Portals;
use mscent_math::Vec3d;

fn v(x: f64, y: f64, z: f64) -> Vec3d {
    Vec3d::new(x, y, z)
}

fn sub(lo: [f64; 3], hi: [f64; 3], order: u32) -> Brush {
    Brush::cuboid(
        v(lo[0], lo[1], lo[2]),
        v(hi[0], hi[1], hi[2]),
        Op::Subtract,
        order,
    )
    .unwrap()
}

fn add(lo: [f64; 3], hi: [f64; 3], order: u32) -> Brush {
    Brush::cuboid(
        v(lo[0], lo[1], lo[2]),
        v(hi[0], hi[1], hi[2]),
        Op::Add,
        order,
    )
    .unwrap()
}

fn empty_leaves(bsp: &Bsp) -> Vec<LeafId> {
    (0u32..)
        .zip(bsp.leaves())
        .filter(|(_, l)| l.kind == LeafKind::Empty)
        .map(|(i, _)| LeafId(i))
        .collect()
}

fn leaf(bsp: &Bsp, x: f64, y: f64, z: f64) -> LeafId {
    bsp.leaf_at(v(x, y, z))
}

#[test]
fn single_room_has_no_portals_and_fills_to_itself() {
    let bsp = compile(&[sub([0.0; 3], [8.0; 3], 0)]).unwrap();
    let portals = Portals::build(&bsp);
    assert!(portals.portals().is_empty());
    let start = leaf(&bsp, 4.0, 4.0, 4.0);
    assert_eq!(portals.flood_fill(start), vec![start]);
}

#[test]
fn two_rooms_and_a_door_connect_through_portals() {
    let bsp = compile(&[
        sub([0.0, 0.0, 0.0], [8.0, 8.0, 8.0], 0),
        sub([16.0, 0.0, 0.0], [24.0, 8.0, 8.0], 1),
        sub([8.0, 3.0, 0.0], [16.0, 5.0, 4.0], 2),
    ])
    .unwrap();
    let portals = Portals::build(&bsp);
    let (a, b) = (leaf(&bsp, 4.0, 4.0, 4.0), leaf(&bsp, 20.0, 4.0, 4.0));
    assert!(!portals.portals().is_empty());
    let reached = portals.flood_fill(a);
    assert!(reached.contains(&b));
    assert_eq!(reached.len(), empty_leaves(&bsp).len());
}

#[test]
fn separate_rooms_are_not_connected() {
    let bsp = compile(&[
        sub([0.0; 3], [8.0; 3], 0),
        sub([20.0, 0.0, 0.0], [28.0, 8.0, 8.0], 1),
    ])
    .unwrap();
    let portals = Portals::build(&bsp);
    let a = leaf(&bsp, 4.0, 4.0, 4.0);
    let b = leaf(&bsp, 24.0, 4.0, 4.0);
    assert!(!portals.flood_fill(a).contains(&b));
}

#[test]
fn rooms_touching_only_along_an_edge_get_no_portal() {
    // The boxes share just the vertical edge x=8, y=8.
    let bsp = compile(&[
        sub([0.0; 3], [8.0; 3], 0),
        sub([8.0, 8.0, 0.0], [16.0, 16.0, 8.0], 1),
    ])
    .unwrap();
    let portals = Portals::build(&bsp);
    assert!(portals.portals().is_empty());
}

#[test]
fn flush_rooms_share_a_portal_with_the_overlap_area() {
    // Rooms share a wall face; the overlap is 8 (y) by 4 (z).
    let bsp = compile(&[
        sub([0.0, 0.0, 0.0], [8.0, 8.0, 8.0], 0),
        sub([8.0, 0.0, 0.0], [16.0, 8.0, 4.0], 1),
    ])
    .unwrap();
    let portals = Portals::build(&bsp);
    let area: f64 = portals.portals().iter().map(|p| p.polygon.area()).sum();
    assert!((area - 32.0).abs() < 1e-6, "area {area}");
}

#[test]
fn portal_ends_are_empty_distinct_and_sides_match_the_normal() {
    let bsp = compile(&[
        sub([0.0; 3], [8.0; 3], 0),
        sub([8.0, 0.0, 0.0], [16.0, 8.0, 4.0], 1),
        add([2.0, 2.0, 0.0], [4.0, 4.0, 8.0], 2),
    ])
    .unwrap();
    let portals = Portals::build(&bsp);
    for p in portals.portals() {
        assert_ne!(p.front, p.back);
        assert_eq!(bsp.leaf_kind(p.front), LeafKind::Empty);
        assert_eq!(bsp.leaf_kind(p.back), LeafKind::Empty);
        let n = p.polygon.normal().unwrap();
        let c = p.polygon.vertices().iter().fold(Vec3d::ZERO, |s, &q| s + q)
            * (1.0 / p.polygon.vertices().len() as f64);
        assert_eq!(bsp.leaf_at(c + n * 0.01), p.front);
        assert_eq!(bsp.leaf_at(c - n * 0.01), p.back);
    }
}

#[test]
fn adjacency_is_symmetric_and_lists_each_portal_at_both_ends() {
    let bsp = compile(&[
        sub([0.0; 3], [8.0; 3], 0),
        sub([8.0, 0.0, 0.0], [16.0, 8.0, 8.0], 1),
        add([4.0, 2.0, 0.0], [12.0, 6.0, 8.0], 2),
    ])
    .unwrap();
    let portals = Portals::build(&bsp);
    for (i, p) in portals.portals().iter().enumerate() {
        let i = i as u32;
        assert!(portals.leaf_portals(p.front).contains(&i));
        assert!(portals.leaf_portals(p.back).contains(&i));
        assert!(portals.neighbors(p.front).any(|n| n == p.back));
        assert!(portals.neighbors(p.back).any(|n| n == p.front));
    }
}

#[test]
fn unknown_and_solid_leaves_are_inert() {
    let bsp = compile(&[sub([0.0; 3], [8.0; 3], 0)]).unwrap();
    let portals = Portals::build(&bsp);
    assert!(portals.flood_fill(LeafId(u32::MAX)).is_empty());
    assert!(portals.leaf_portals(LeafId(u32::MAX)).is_empty());
    let solid = leaf(&bsp, 100.0, 0.0, 0.0);
    assert_eq!(portals.flood_fill(solid), vec![solid]);
}
