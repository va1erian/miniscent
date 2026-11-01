use crate::bsp::{Bsp, BspError, Child, Leaf, LeafId, LeafKind, MAX_DEPTH, Node, NodeId, Region};
use mscent_math::{Aabb, DPlane, Vec3, Vec3d};

const SOLID: Leaf = Leaf {
    kind: LeafKind::Solid,
};
const EMPTY: Leaf = Leaf {
    kind: LeafKind::Empty,
};

fn plane(x: f64, y: f64, z: f64, d: f64) -> DPlane {
    DPlane::new(Vec3d::new(x, y, z), d)
}

/// x > 0 is empty (leaf 1), x <= 0 solid (leaf 0); the empty half is cut at
/// y = 10 into empty (front... back) so leaf 2 is solid for y > 10.
fn two_split() -> Bsp {
    let nodes = vec![
        Node {
            plane: plane(1.0, 0.0, 0.0, 0.0),
            front: Child::Node(NodeId(1)),
            back: Child::Leaf(LeafId(0)),
        },
        Node {
            plane: plane(0.0, 1.0, 0.0, 10.0),
            front: Child::Leaf(LeafId(2)),
            back: Child::Leaf(LeafId(1)),
        },
    ];
    Bsp::from_parts(nodes, vec![SOLID, EMPTY, SOLID], Child::Node(NodeId(0))).unwrap()
}

#[test]
fn classifies_points() {
    let t = two_split();
    let at = |x, y, z| t.leaf_at(Vec3d::new(x, y, z));
    assert_eq!(at(-1.0, 0.0, 0.0), LeafId(0));
    assert_eq!(at(1.0, 5.0, 0.0), LeafId(1));
    assert_eq!(at(1.0, 15.0, 0.0), LeafId(2));
    assert!(t.is_solid(Vec3d::new(-5.0, 0.0, 0.0)));
    assert!(!t.is_solid(Vec3d::new(5.0, 0.0, 0.0)));
    // On the plane goes back.
    assert_eq!(at(0.0, 0.0, 0.0), LeafId(0));
    assert_eq!(t.leaf_at_f32(Vec3::new(1.0, 5.0, 0.0)), LeafId(1));
    assert_eq!(t.depth(), 2);
}

#[test]
fn classifies_boxes_and_spheres() {
    let t = two_split();
    let b = |lo: [f32; 3], hi: [f32; 3]| {
        Aabb::new(
            Vec3::new(lo[0], lo[1], lo[2]),
            Vec3::new(hi[0], hi[1], hi[2]),
        )
    };
    assert_eq!(t.classify_aabb(&b([-3.0; 3], [-1.0; 3])), Region::Solid);
    assert_eq!(
        t.classify_aabb(&b([1.0, 1.0, 0.0], [2.0, 2.0, 1.0])),
        Region::Empty
    );
    assert_eq!(
        t.classify_aabb(&b([-1.0, 1.0, 0.0], [2.0, 2.0, 1.0])),
        Region::Mixed
    );
    assert_eq!(
        t.classify_aabb(&b([1.0, 9.0, 0.0], [2.0, 11.0, 1.0])),
        Region::Mixed
    );
    let c = |x, y, z, r| t.classify_sphere(Vec3::new(x, y, z), r);
    assert_eq!(c(5.0, 5.0, 0.0, 1.0), Region::Empty);
    assert_eq!(c(0.5, 5.0, 0.0, 1.0), Region::Mixed);
    assert_eq!(c(-5.0, 5.0, 0.0, 1.0), Region::Solid);
    assert_eq!(c(5.0, 10.5, 0.0, 1.0), Region::Mixed);
}

#[test]
fn single_leaf_tree() {
    let t = Bsp::from_parts(vec![], vec![EMPTY], Child::Leaf(LeafId(0))).unwrap();
    assert!(!t.is_solid(Vec3d::new(1.0, 2.0, 3.0)));
    assert_eq!(t.depth(), 0);
}

#[test]
fn rejects_hostile_trees() {
    let n = |front, back| Node {
        plane: plane(1.0, 0.0, 0.0, 0.0),
        front,
        back,
    };
    let l = |i| Child::Leaf(LeafId(i));
    let nd = |i| Child::Node(NodeId(i));
    let leaves = vec![SOLID, EMPTY];
    let err = |nodes: Vec<Node>, leaves: Vec<Leaf>, root| {
        Bsp::from_parts(nodes, leaves, root).unwrap_err()
    };
    assert_eq!(err(vec![], leaves.clone(), l(5)), BspError::IndexOutOfRange);
    assert_eq!(
        err(vec![n(l(0), l(9))], leaves.clone(), nd(0)),
        BspError::IndexOutOfRange
    );
    assert_eq!(
        err(vec![n(l(0), l(0))], leaves.clone(), nd(0)),
        BspError::Shared
    );
    assert_eq!(
        err(vec![n(nd(0), l(0))], leaves.clone(), nd(0)),
        BspError::Shared
    );
    assert_eq!(
        err(vec![n(l(0), l(1))], leaves.clone(), l(0)),
        BspError::Unreachable
    );
    let mut bad = n(l(0), l(1));
    bad.plane.dist = f64::NAN;
    assert_eq!(err(vec![bad], leaves, nd(0)), BspError::NotFinite);
}

#[test]
fn rejects_too_deep_and_accepts_max_depth() {
    let chain = |len: usize| {
        let mut nodes = Vec::new();
        let mut leaves = vec![SOLID];
        for i in 0..len {
            leaves.push(EMPTY);
            let back = if i + 1 == len {
                Child::Leaf(LeafId(0))
            } else {
                Child::Node(NodeId(i as u32 + 1))
            };
            nodes.push(Node {
                plane: plane(1.0, 0.0, 0.0, i as f64),
                front: Child::Leaf(LeafId(i as u32 + 1)),
                back,
            });
        }
        Bsp::from_parts(nodes, leaves, Child::Node(NodeId(0)))
    };
    assert!(chain(MAX_DEPTH).is_ok());
    assert_eq!(chain(MAX_DEPTH + 1).unwrap_err(), BspError::TooDeep);
    // A very deep hostile tree must not overflow the stack.
    assert_eq!(chain(100_000).unwrap_err(), BspError::TooDeep);
}
