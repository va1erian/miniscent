use crate::brush::{Brush, Op};
use crate::bsp::{Bsp, LeafId, LeafKind};
use crate::csg::compile;
use crate::r#gen::{GenParams, generate};
use crate::islands::Islands;
use crate::portals::Portals;
use mscent_math::Vec3d;

fn sub(lo: [f64; 3], hi: [f64; 3], order: u32) -> Brush {
    let v = |a: [f64; 3]| Vec3d::new(a[0], a[1], a[2]);
    Brush::cuboid(v(lo), v(hi), Op::Subtract, order).unwrap()
}

fn built(brushes: &[Brush]) -> (Bsp, Portals, Islands) {
    let bsp = compile(brushes).unwrap();
    let portals = Portals::build(&bsp);
    let islands = Islands::build(&bsp, &portals);
    (bsp, portals, islands)
}

fn at(bsp: &Bsp, x: f64, y: f64, z: f64) -> LeafId {
    bsp.leaf_at(Vec3d::new(x, y, z))
}

fn room_a() -> Brush {
    sub([0.0; 3], [8.0; 3], 0)
}

fn room_b() -> Brush {
    sub([16.0, 0.0, 0.0], [24.0, 8.0, 8.0], 1)
}

#[test]
fn two_separate_rooms_are_two_islands() {
    let (bsp, _, islands) = built(&[room_a(), room_b()]);
    assert_eq!(islands.count(), 2);
    let (a, b) = (at(&bsp, 4.0, 4.0, 4.0), at(&bsp, 20.0, 4.0, 4.0));
    assert_ne!(islands.island_of(a), islands.island_of(b));
    assert!(islands.island_of(a).is_some() && islands.island_of(b).is_some());
}

#[test]
fn rooms_joined_by_a_door_are_one_island() {
    let door = sub([8.0, 3.0, 0.0], [16.0, 5.0, 4.0], 2);
    let (bsp, _, islands) = built(&[room_a(), room_b(), door]);
    assert_eq!(islands.count(), 1);
    let (a, b) = (at(&bsp, 4.0, 4.0, 4.0), at(&bsp, 20.0, 4.0, 4.0));
    assert_eq!(islands.island_of(a), islands.island_of(b));
    // The island lists every empty leaf and nothing else.
    let empties = (bsp.leaves().iter())
        .filter(|l| l.kind == LeafKind::Empty)
        .count();
    assert_eq!(islands.leaves(0).len(), empties);
}

#[test]
fn solid_and_unknown_leaves_have_no_island() {
    let (bsp, _, islands) = built(&[room_a()]);
    assert_eq!(islands.island_of(at(&bsp, -50.0, 0.0, 0.0)), None);
    assert_eq!(islands.island_of(LeafId(u32::MAX)), None);
    assert!(islands.leaves(7).is_empty());
}

#[test]
fn a_level_with_no_empty_space_has_no_islands() {
    let (_, _, islands) = built(&[]);
    assert_eq!(islands.count(), 0);
}

fn root(parent: &mut [usize], mut i: usize) -> usize {
    while parent[i] != i {
        parent[i] = parent[parent[i]];
        i = parent[i];
    }
    i
}

/// Islands by an independent route: union-find over the portal list.
fn union_find_classes(bsp: &Bsp, portals: &Portals) -> Vec<usize> {
    let mut parent: Vec<usize> = (0..bsp.leaves().len()).collect();
    for p in portals.portals() {
        let a = root(&mut parent, p.front.0 as usize);
        let b = root(&mut parent, p.back.0 as usize);
        parent[a.max(b)] = a.min(b);
    }
    (0..parent.len()).map(|i| root(&mut parent, i)).collect()
}

#[test]
fn islands_agree_with_union_find_on_generated_levels() {
    for seed in 0..12u64 {
        let params = GenParams {
            cols: 2 + u32::try_from(seed % 3).unwrap(),
            rows: 2,
            extra_links: u32::try_from(seed % 4).unwrap(),
            pillar_percent: 50,
        };
        let mut brushes = generate(seed, &params);
        // A sealed room far outside the grid, so there is more than one island.
        let order = u32::try_from(brushes.len()).unwrap();
        brushes.push(sub([1000.0; 3], [1008.0; 3], order));
        let (bsp, portals, islands) = built(&brushes);
        assert_eq!(islands.count(), 2, "seed {seed}");
        for p in portals.portals() {
            let (a, b) = (islands.island_of(p.front), islands.island_of(p.back));
            assert!(a.is_some() && a == b, "seed {seed}");
        }
        let classes = union_find_classes(&bsp, &portals);
        let empties: Vec<usize> = (0..bsp.leaves().len())
            .filter(|&i| bsp.leaves()[i].kind == LeafKind::Empty)
            .collect();
        let mut roots: Vec<usize> = empties.iter().map(|&i| classes[i]).collect();
        roots.sort_unstable();
        roots.dedup();
        assert_eq!(roots.len(), islands.count() as usize, "seed {seed}");
        // Same partition: two leaves share a class exactly when they share an island.
        let id = |i: usize| LeafId(u32::try_from(i).unwrap());
        for &i in &empties {
            for &j in &empties {
                let same = islands.island_of(id(i)) == islands.island_of(id(j));
                assert_eq!(classes[i] == classes[j], same, "seed {seed}");
            }
        }
        let total: usize = (0..islands.count()).map(|k| islands.leaves(k).len()).sum();
        assert_eq!(total, empties.len(), "seed {seed}");
    }
}
