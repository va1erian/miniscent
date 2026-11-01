use super::*;
use crate::brush::{Brush, Op};
use crate::bsp::LeafKind;
use crate::csg::compile;
use mscent_math::{DPlane, Vec3d};

const EPS: f64 = 1.0e-6;

fn v(x: f64, y: f64, z: f64) -> Vec3d {
    Vec3d::new(x, y, z)
}

fn boxb(lo: [f64; 3], hi: [f64; 3], op: Op, order: u32) -> Brush {
    Brush::cuboid(v(lo[0], lo[1], lo[2]), v(hi[0], hi[1], hi[2]), op, order).unwrap()
}

fn sub(lo: [f64; 3], hi: [f64; 3], order: u32) -> Brush {
    boxb(lo, hi, Op::Subtract, order)
}

fn extract(brushes: &[Brush]) -> Vec<Face> {
    extract_faces(&compile(brushes).unwrap(), brushes)
}

/// Total area of faces lying on `normal . p = dist` (empty-side normal).
fn area_on(faces: &[Face], normal: [f64; 3], dist: f64) -> f64 {
    let plane = DPlane::new(v(normal[0], normal[1], normal[2]), dist);
    faces
        .iter()
        .filter(|f| super::same_plane(&f.plane, &plane))
        .map(|f| f.polygon.area())
        .sum()
}

fn total_area(faces: &[Face]) -> f64 {
    faces.iter().map(|f| f.polygon.area()).sum()
}

fn assert_area(faces: &[Face], normal: [f64; 3], dist: f64, want: f64) {
    let got = area_on(faces, normal, dist);
    assert!(
        (got - want).abs() < EPS,
        "plane {normal:?} d={dist}: {got} != {want}"
    );
}

fn contains_point(f: &Face, p: Vec3d) -> bool {
    let vs = f.polygon.vertices();
    let n = f.plane.normal;
    (0..vs.len()).all(|i| {
        let (a, b) = (vs[i], vs[(i + 1) % vs.len()]);
        (b - a).cross(p - a).dot(n) >= 0.0
    })
}

/// Every directed edge must be matched by the reverse edge of a neighbour.
fn assert_watertight(faces: &[Face]) {
    let edges: Vec<(Vec3d, Vec3d)> = faces
        .iter()
        .flat_map(|f| {
            let vs = f.polygon.vertices();
            (0..vs.len()).map(move |i| (vs[i], vs[(i + 1) % vs.len()]))
        })
        .collect();
    let near = |a: Vec3d, b: Vec3d| (a - b).length() < EPS;
    for &(a, b) in &edges {
        assert!(
            edges.iter().any(|&(c, d)| near(c, b) && near(d, a)),
            "edge {a:?} -> {b:?} has no partner"
        );
    }
}

fn assert_windings(faces: &[Face]) {
    for f in faces {
        let n = f.polygon.normal().unwrap();
        assert!(
            (n - f.plane.normal).length() < EPS,
            "winding disagrees with plane"
        );
        for p in f.polygon.vertices() {
            assert!(f.plane.signed_distance(*p).abs() < EPS);
        }
    }
}

fn door_room() -> Vec<Brush> {
    vec![
        sub([0.0; 3], [10.0; 3], 0),
        sub([10.0, 4.0, 0.0], [14.0, 6.0, 3.0], 1),
    ]
}

#[test]
fn no_geometry_no_faces() {
    assert!(extract(&[]).is_empty());
}

#[test]
fn room_has_six_full_faces() {
    let brushes = [sub([0.0; 3], [10.0; 3], 0)];
    let bsp = compile(&brushes).unwrap();
    let faces = extract_faces(&bsp, &brushes);
    assert_eq!(faces.len(), 6);
    assert_area(&faces, [0.0, 0.0, 1.0], 0.0, 100.0);
    assert_area(&faces, [0.0, 0.0, -1.0], -10.0, 100.0);
    assert_area(&faces, [1.0, 0.0, 0.0], 0.0, 100.0);
    assert_area(&faces, [-1.0, 0.0, 0.0], -10.0, 100.0);
    assert_area(&faces, [0.0, 1.0, 0.0], 0.0, 100.0);
    assert_area(&faces, [0.0, -1.0, 0.0], -10.0, 100.0);
    assert_windings(&faces);
    assert_watertight(&faces);
    for f in &faces {
        assert_eq!(bsp.leaf_kind(f.front_leaves[0]), LeafKind::Empty);
        assert_eq!(bsp.leaf_kind(f.back_leaves[0]), LeafKind::Solid);
    }
}

#[test]
fn room_with_door_tiles_each_surface() {
    let faces = extract(&door_room());
    assert_windings(&faces);
    assert_watertight(&faces);
    // Floor: room plus corridor; ceiling splits into room and corridor
    // heights, so each plane carries its own analytic area.
    assert_area(&faces, [0.0, 0.0, 1.0], 0.0, 108.0);
    assert_area(&faces, [0.0, 0.0, -1.0], -10.0, 100.0);
    assert_area(&faces, [0.0, 0.0, -1.0], -3.0, 8.0);
    assert_area(&faces, [-1.0, 0.0, 0.0], -10.0, 94.0);
    assert_area(&faces, [1.0, 0.0, 0.0], 0.0, 100.0);
    assert_area(&faces, [-1.0, 0.0, 0.0], -14.0, 6.0);
    assert_area(&faces, [0.0, 1.0, 0.0], 0.0, 100.0);
    assert_area(&faces, [0.0, -1.0, 0.0], -10.0, 100.0);
    assert_area(&faces, [0.0, 1.0, 0.0], 4.0, 12.0);
    assert_area(&faces, [0.0, -1.0, 0.0], -6.0, 12.0);
    assert!((total_area(&faces) - 640.0).abs() < EPS);
}

#[test]
fn door_room_floor_is_covered_exactly_once() {
    let faces = extract(&door_room());
    let floor: Vec<&Face> = faces
        .iter()
        .filter(|f| f.plane.normal == v(0.0, 0.0, 1.0) && f.plane.dist == 0.0)
        .collect();
    let mut y = 0.31;
    while y < 10.0 {
        let mut x = 0.37;
        while x < 14.0 {
            let p = v(x, y, 0.0);
            let inside = x < 10.0 || (4.0..6.0).contains(&y);
            let hits = floor.iter().filter(|f| contains_point(f, p)).count();
            assert_eq!(hits, usize::from(inside), "at {p:?}");
            x += 0.5;
        }
        y += 0.5;
    }
}

#[test]
fn wall_with_door_has_a_hole() {
    let faces = extract(&door_room());
    let wall: Vec<&Face> = faces
        .iter()
        .filter(|f| f.plane.normal == v(-1.0, 0.0, 0.0) && f.plane.dist == -10.0)
        .collect();
    for (y, z, solid) in [(5.0, 1.5, false), (5.0, 5.0, true), (2.0, 1.5, true)] {
        let p = v(10.0, y, z);
        assert_eq!(wall.iter().any(|f| contains_point(f, p)), solid, "at {p:?}");
    }
}

#[test]
fn touching_rooms_merge_into_one_floor() {
    let brushes = [
        sub([0.0; 3], [10.0; 3], 0),
        sub([5.0, 0.0, 0.0], [15.0, 10.0, 10.0], 1),
    ];
    let bsp = compile(&brushes).unwrap();
    let faces = extract_faces(&bsp, &brushes);
    let floors: Vec<&Face> = faces
        .iter()
        .filter(|f| f.plane.normal == v(0.0, 0.0, 1.0))
        .collect();
    assert_eq!(floors.len(), 1, "coplanar neighbours must merge");
    assert!((floors[0].polygon.area() - 150.0).abs() < EPS);
    assert!(floors[0].front_leaves.len() >= 2);
    assert_watertight(&faces);
}

#[test]
fn different_materials_do_not_merge_and_are_assigned() {
    let brushes = [
        sub([0.0; 3], [10.0; 3], 0).with_material(2),
        sub([10.0, 0.0, 0.0], [20.0, 10.0, 10.0], 1).with_material(7),
    ];
    let faces = extract(&brushes);
    let by_material = |m: u16| -> f64 {
        faces
            .iter()
            .filter(|f| f.material == m && f.plane.normal == v(0.0, 0.0, 1.0))
            .map(|f| f.polygon.area())
            .sum()
    };
    assert!((by_material(2) - 100.0).abs() < EPS);
    assert!((by_material(7) - 100.0).abs() < EPS);
    assert_eq!(
        faces
            .iter()
            .filter(|f| f.plane.normal == v(0.0, 0.0, 1.0))
            .count(),
        2
    );
}

#[test]
fn pillar_faces_get_the_add_brush_material() {
    let brushes = [
        sub([0.0; 3], [10.0; 3], 0).with_material(1),
        boxb([4.0, 4.0, 0.0], [6.0, 6.0, 10.0], Op::Add, 1).with_material(9),
    ];
    let faces = extract(&brushes);
    assert_windings(&faces);
    assert_watertight(&faces);
    assert_area(&faces, [0.0, 0.0, 1.0], 0.0, 96.0);
    assert_area(&faces, [0.0, 0.0, -1.0], -10.0, 96.0);
    // Pillar sides face outwards from the pillar into the room.
    assert_area(&faces, [-1.0, 0.0, 0.0], -4.0, 20.0);
    assert_area(&faces, [1.0, 0.0, 0.0], 6.0, 20.0);
    let pillar: f64 = faces
        .iter()
        .filter(|f| f.material == 9)
        .map(|f| f.polygon.area())
        .sum();
    assert!((pillar - 80.0).abs() < EPS);
}

#[test]
fn t_junctions_are_split() {
    // A pillar standing on the floor leaves floor vertices inside the
    // walls' bottom edges only if not repaired; here the pillar touches the
    // east wall, so the wall edge must pick up the pillar's corners.
    let brushes = [
        sub([0.0; 3], [10.0; 3], 0),
        boxb([8.0, 4.0, 0.0], [10.0, 6.0, 10.0], Op::Add, 1),
    ];
    let faces = extract(&brushes);
    assert_watertight(&faces);
    assert_area(&faces, [0.0, 0.0, 1.0], 0.0, 96.0);
}

#[test]
fn mismatched_brushes_only_lose_materials() {
    let brushes = [sub([0.0; 3], [10.0; 3], 0).with_material(5)];
    let bsp = compile(&brushes).unwrap();
    let faces = extract_faces(&bsp, &[]);
    assert_eq!(faces.len(), 6);
    assert!(faces.iter().all(|f| f.material == 0));
}
