use crate::brush::{Brush, Op};
use crate::bsp::{Bsp, LeafKind};
use crate::csg::compile;
use mscent_math::rng::Rng;
use mscent_math::{DPlane, Vec3d};

fn v(x: f64, y: f64, z: f64) -> Vec3d {
    Vec3d::new(x, y, z)
}

fn boxb(lo: [f64; 3], hi: [f64; 3], op: Op, order: u32) -> Brush {
    Brush::cuboid(v(lo[0], lo[1], lo[2]), v(hi[0], hi[1], hi[2]), op, order).unwrap()
}

fn sub(lo: [f64; 3], hi: [f64; 3], order: u32) -> Brush {
    boxb(lo, hi, Op::Subtract, order)
}

fn add(lo: [f64; 3], hi: [f64; 3], order: u32) -> Brush {
    boxb(lo, hi, Op::Add, order)
}

/// Reference semantics: the last brush (by order, then slice position) that
/// contains the point decides; no brush means solid.
fn oracle(brushes: &[Brush], p: Vec3d) -> bool {
    let mut sorted: Vec<&Brush> = brushes.iter().collect();
    sorted.sort_by_key(|b| b.order());
    sorted
        .iter()
        .rev()
        .find(|b| b.contains(p))
        .is_none_or(|b| b.op() == Op::Add)
}

fn min_plane_gap(brushes: &[Brush], p: Vec3d) -> f64 {
    brushes
        .iter()
        .flat_map(|b| b.planes().iter())
        .map(|pl| pl.signed_distance(p).abs())
        .fold(f64::MAX, f64::min)
}

/// Compares the BSP with the oracle on a lattice offset from all cuboid planes.
fn check_lattice(brushes: &[Brush], bsp: &Bsp, lo: f64, hi: f64) {
    let mut x = lo + 0.0625;
    while x < hi {
        let mut y = lo + 0.0625;
        while y < hi {
            let mut z = lo + 0.0625;
            while z < hi {
                let p = v(x, y, z);
                assert_eq!(bsp.is_solid(p), oracle(brushes, p), "at {p:?}");
                z += 0.75;
            }
            y += 0.75;
        }
        x += 0.75;
    }
}

fn empty_leaves(bsp: &Bsp) -> usize {
    bsp.leaves()
        .iter()
        .filter(|l| l.kind == LeafKind::Empty)
        .count()
}

#[test]
fn no_brushes_is_solid() {
    let bsp = compile(&[]).unwrap();
    assert!(bsp.is_solid(v(0.0, 0.0, 0.0)));
    assert!(bsp.nodes().is_empty());
}

#[test]
fn room() {
    let brushes = [sub([0.0; 3], [10.0; 3], 0)];
    let bsp = compile(&brushes).unwrap();
    assert!(!bsp.is_solid(v(5.0, 5.0, 5.0)));
    assert!(bsp.is_solid(v(-1.0, 5.0, 5.0)));
    assert!(bsp.is_solid(v(5.0, 5.0, 10.5)));
    assert_eq!(bsp.nodes().len(), 6);
    assert_eq!(empty_leaves(&bsp), 1);
    check_lattice(&brushes, &bsp, -3.0, 13.0);
}

#[test]
fn room_with_door() {
    let brushes = [
        sub([0.0; 3], [10.0; 3], 0),
        sub([9.0, 4.0, 0.0], [16.0, 6.0, 4.0], 1),
    ];
    let bsp = compile(&brushes).unwrap();
    assert!(!bsp.is_solid(v(13.0, 5.0, 2.0)), "inside the corridor");
    assert!(bsp.is_solid(v(13.0, 8.0, 2.0)), "beside the corridor");
    assert!(!bsp.is_solid(v(9.5, 5.0, 2.0)), "doorway");
    assert!(bsp.is_solid(v(17.0, 5.0, 2.0)), "past the corridor end");
    check_lattice(&brushes, &bsp, -3.0, 19.0);
}

#[test]
fn nested_subtract_inside_existing_room_changes_nothing() {
    let outer = [sub([0.0; 3], [10.0; 3], 0)];
    let nested = [
        sub([0.0; 3], [10.0; 3], 0),
        sub([2.0; 3], [4.0; 3], 1),
        sub([5.0; 3], [8.0; 3], 2),
    ];
    let a = compile(&outer).unwrap();
    let b = compile(&nested).unwrap();
    assert_eq!(a.nodes().len(), b.nodes().len());
    assert_eq!(empty_leaves(&b), 1);
    check_lattice(&nested, &b, -2.0, 12.0);
}

#[test]
fn overlapping_subtracts() {
    let brushes = [
        sub([0.0; 3], [6.0; 3], 0),
        sub([4.0; 3], [10.0; 3], 1),
        sub([-2.0, 2.0, 2.0], [3.0, 3.0, 3.0], 2),
    ];
    let bsp = compile(&brushes).unwrap();
    check_lattice(&brushes, &bsp, -4.0, 12.0);
    assert!(!bsp.is_solid(v(8.0, 8.0, 8.0)));
    assert!(bsp.is_solid(v(8.0, 1.0, 1.0)));
}

#[test]
fn order_sensitivity() {
    let room = [0.0; 3];
    let big = [10.0; 3];
    let block = ([4.0; 3], [14.0; 3]);
    // Subtract then add: the block refills part of the room.
    let sub_then_add = [sub(room, big, 0), add(block.0, block.1, 1)];
    // Add then subtract: the room carves through the block.
    let add_then_sub = [add(block.0, block.1, 0), sub(room, big, 1)];
    let a = compile(&sub_then_add).unwrap();
    let b = compile(&add_then_sub).unwrap();
    let p = v(6.0, 6.0, 6.0); // in both brushes
    assert!(a.is_solid(p));
    assert!(!b.is_solid(p));
    let q = v(12.0, 12.0, 12.0); // only in the block
    assert!(a.is_solid(q) && b.is_solid(q));
    let r = v(1.0, 1.0, 1.0); // only in the room
    assert!(!a.is_solid(r) && !b.is_solid(r));
    check_lattice(&sub_then_add, &a, -2.0, 16.0);
    check_lattice(&add_then_sub, &b, -2.0, 16.0);
}

#[test]
fn order_field_beats_slice_position() {
    let brushes = [add([4.0; 3], [14.0; 3], 1), sub([0.0; 3], [10.0; 3], 0)];
    let bsp = compile(&brushes).unwrap();
    assert!(bsp.is_solid(v(6.0, 6.0, 6.0)));
    check_lattice(&brushes, &bsp, -2.0, 16.0);
}

#[test]
fn coplanar_adjacent_rooms_merge() {
    // The shared wall at x = 4 has zero thickness, so the rooms are one space.
    let brushes = [
        sub([0.0; 3], [4.0; 3], 0),
        sub([4.0, 0.0, 0.0], [8.0, 4.0, 4.0], 1),
    ];
    let bsp = compile(&brushes).unwrap();
    for x in [3.9, 4.1] {
        assert!(!bsp.is_solid(v(x, 2.0, 2.0)));
    }
    check_lattice(&brushes, &bsp, -2.0, 10.0);
}

#[test]
fn coplanar_faces_with_floor_and_identical_brushes() {
    // Pillar flush with the floor and a duplicate of the room itself.
    let brushes = [
        sub([0.0; 3], [10.0; 3], 0),
        sub([0.0; 3], [10.0; 3], 1),
        add([4.0, 4.0, 0.0], [6.0, 6.0, 10.0], 2),
        add([4.0, 4.0, 0.0], [6.0, 6.0, 10.0], 3),
    ];
    let bsp = compile(&brushes).unwrap();
    assert!(bsp.is_solid(v(5.0, 5.0, 5.0)));
    assert!(!bsp.is_solid(v(2.0, 2.0, 5.0)));
    check_lattice(&brushes, &bsp, -2.0, 12.0);
}

#[test]
fn pillar_inside_room() {
    let brushes = [
        sub([0.0; 3], [12.0; 3], 0),
        add([5.0, 5.0, 0.0], [7.0, 7.0, 12.0], 1),
    ];
    let bsp = compile(&brushes).unwrap();
    assert!(bsp.is_solid(v(6.0, 6.0, 6.0)));
    assert!(!bsp.is_solid(v(3.0, 6.0, 6.0)));
    assert!(!bsp.is_solid(v(9.0, 6.0, 6.0)));
    check_lattice(&brushes, &bsp, -2.0, 14.0);
}

#[test]
fn rejects_over_deep_inputs_gracefully() {
    // Many staggered cuboids force deep trees; the build either succeeds
    // with bounded depth or reports an error, never panics.
    let brushes: Vec<Brush> = (0..40)
        .map(|i| {
            let o = f64::from(i) * 0.5;
            sub(
                [o, o * 0.5, 0.0],
                [o + 3.0, o * 0.5 + 3.0, 3.0 + o],
                i as u32,
            )
        })
        .collect();
    match compile(&brushes) {
        Ok(bsp) => assert!(bsp.depth() <= crate::bsp::MAX_DEPTH),
        Err(e) => assert_eq!(e, crate::csg::CsgError::TooDeep),
    }
}

fn random_cuboid(rng: &mut Rng, order: u32) -> Brush {
    // Multiples of 0.5 inside [-8, 8], at least one cell wide.
    let mut lo = [0.0; 3];
    let mut hi = [0.0; 3];
    for a in 0..3 {
        let s = f64::from(rng.below(24)) * 0.5 - 8.0;
        lo[a] = s;
        hi[a] = s + 0.5 + f64::from(rng.below(10)) * 0.5;
    }
    let op = if rng.below(3) == 0 {
        Op::Add
    } else {
        Op::Subtract
    };
    boxb(lo, hi, op, order)
}

fn random_convex(rng: &mut Rng, order: u32) -> Option<Brush> {
    let centre = v(
        rng.range_f64(-5.0, 5.0),
        rng.range_f64(-5.0, 5.0),
        rng.range_f64(-5.0, 5.0),
    );
    let planes: Vec<DPlane> = (0..4 + rng.below(5))
        .filter_map(|_| {
            let n = v(
                rng.range_f64(-1.0, 1.0),
                rng.range_f64(-1.0, 1.0),
                rng.range_f64(-1.0, 1.0),
            )
            .normalized()?;
            Some(DPlane::new(n, n.dot(centre) + rng.range_f64(1.0, 5.0)))
        })
        .collect();
    let op = if rng.below(3) == 0 {
        Op::Add
    } else {
        Op::Subtract
    };
    Brush::new(&planes, op, order).ok()
}

fn compare_random(brushes: &[Brush], bsp: &Bsp, rng: &mut Rng, samples: usize) -> usize {
    let mut checked = 0;
    for _ in 0..samples {
        let p = v(
            rng.range_f64(-14.0, 14.0),
            rng.range_f64(-14.0, 14.0),
            rng.range_f64(-14.0, 14.0),
        );
        if min_plane_gap(brushes, p) < 1.0e-3 {
            continue;
        }
        assert_eq!(bsp.is_solid(p), oracle(brushes, p), "point {p:?}");
        checked += 1;
    }
    checked
}

#[test]
fn property_random_cuboids_match_ordered_evaluation() {
    let mut rng = Rng::new(0xC5C);
    for round in 0..60 {
        let n = 1 + rng.below(6);
        let brushes: Vec<Brush> = (0..n).map(|i| random_cuboid(&mut rng, i)).collect();
        let bsp = compile(&brushes).unwrap_or_else(|e| panic!("round {round}: {e:?}"));
        assert!(compare_random(&brushes, &bsp, &mut rng, 300) > 200);
    }
}

#[test]
fn property_random_convex_brushes_match_ordered_evaluation() {
    let mut rng = Rng::new(0xFACE);
    let mut total = 0;
    for _ in 0..40 {
        let mut brushes = Vec::new();
        for i in 0..1 + rng.below(5) {
            brushes.extend(random_convex(&mut rng, i));
        }
        let bsp = compile(&brushes).unwrap();
        total += compare_random(&brushes, &bsp, &mut rng, 300);
    }
    assert!(total > 2000, "only {total} points checked");
}

#[test]
fn property_shuffled_slice_order_gives_same_classification() {
    let mut rng = Rng::new(5);
    for _ in 0..20 {
        let brushes: Vec<Brush> = (0..5).map(|i| random_cuboid(&mut rng, i)).collect();
        let mut reversed = brushes.clone();
        reversed.reverse();
        let (a, b) = (compile(&brushes).unwrap(), compile(&reversed).unwrap());
        for _ in 0..200 {
            let p = v(
                rng.range_f64(-14.0, 14.0),
                rng.range_f64(-14.0, 14.0),
                rng.range_f64(-14.0, 14.0),
            );
            if min_plane_gap(&brushes, p) >= 1.0e-3 {
                assert_eq!(a.is_solid(p), b.is_solid(p));
            }
        }
    }
}
