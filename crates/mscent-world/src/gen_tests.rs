use crate::brush::Op;
use crate::bsp::{Bsp, LeafKind};
use crate::csg::compile;
use crate::r#gen::{GenParams, generate, room_centers};
use crate::portals::Portals;

fn check_level(seed: u64, params: &GenParams) -> (Bsp, Portals) {
    let brushes = generate(seed, params);
    let bsp = compile(&brushes).unwrap_or_else(|e| panic!("seed {seed}: compile failed {e:?}"));
    let portals = Portals::build(&bsp);
    let centers = room_centers(seed, params);
    let first = bsp.leaf_at(centers[0]);
    let reached = portals.flood_fill(first);
    for c in &centers {
        let leaf = bsp.leaf_at(*c);
        assert_eq!(
            bsp.leaf_kind(leaf),
            LeafKind::Empty,
            "seed {seed}: room centre is solid"
        );
        assert!(reached.contains(&leaf), "seed {seed}: room unreachable");
    }
    // Every empty leaf, not just the room centres, belongs to the one component.
    let empties = bsp
        .leaves()
        .iter()
        .filter(|l| l.kind == LeafKind::Empty)
        .count();
    assert_eq!(
        reached.len(),
        empties,
        "seed {seed}: disconnected empty leaves"
    );
    (bsp, portals)
}

#[test]
fn same_seed_same_level() {
    let p = GenParams::default();
    assert_eq!(generate(42, &p), generate(42, &p));
    assert_ne!(generate(42, &p), generate(43, &p));
}

#[test]
fn brush_counts_and_kinds_are_sane() {
    let p = GenParams {
        pillar_percent: 100,
        ..GenParams::default()
    };
    let b = generate(7, &p);
    assert_eq!(b.iter().filter(|x| x.op() == Op::Add).count(), 9);
    assert!(b.iter().filter(|x| x.op() == Op::Subtract).count() >= 9 + 2 * 8);
    assert!(b.windows(2).all(|w| w[0].order() < w[1].order()));
}

#[test]
fn two_hundred_seeds_compile_and_connect() {
    for seed in 0..200u64 {
        let params = GenParams {
            cols: 1 + (seed % 3) as u32,
            rows: 1 + (seed / 3 % 3) as u32,
            extra_links: (seed % 4) as u32,
            pillar_percent: [0, 50, 100][(seed % 3) as usize],
        };
        check_level(seed, &params);
    }
}

#[test]
fn out_of_range_params_are_clamped() {
    let p = GenParams {
        cols: 0,
        rows: 1000,
        extra_links: 3,
        pillar_percent: 500,
    };
    let brushes = generate(1, &p);
    assert!(!brushes.is_empty());
    assert_eq!(room_centers(1, &p).len(), 8);
}

#[test]
fn larger_level_connects() {
    let p = GenParams {
        cols: 4,
        rows: 4,
        extra_links: 4,
        pillar_percent: 60,
    };
    let (_, portals) = check_level(5, &p);
    assert!(!portals.portals().is_empty());
}
