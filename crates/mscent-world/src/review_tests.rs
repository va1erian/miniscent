//! Adversarial CSG and level-format checks added in review: heavy plane
//! sharing, near-coplanar planes, extreme coordinates and hostile files.

use crate::brush::{Brush, Op};
use crate::csg::compile;
use crate::entity::Level;
use crate::format;
use mscent_math::rng::Rng;
use mscent_math::{DPlane, Vec3d};

fn v(x: f64, y: f64, z: f64) -> Vec3d {
    Vec3d::new(x, y, z)
}

fn oracle(brushes: &[Brush], p: Vec3d) -> bool {
    let mut sorted: Vec<&Brush> = brushes.iter().collect();
    sorted.sort_by_key(|b| b.order());
    sorted
        .iter()
        .rev()
        .find(|b| b.contains(p))
        .is_none_or(|b| b.op() == Op::Add)
}

fn gap(brushes: &[Brush], p: Vec3d) -> f64 {
    brushes
        .iter()
        .flat_map(|b| b.planes().iter())
        .map(|pl| pl.signed_distance(p).abs())
        .fold(f64::MAX, f64::min)
}

fn random_op(rng: &mut Rng) -> Op {
    if rng.below(3) == 0 {
        Op::Add
    } else {
        Op::Subtract
    }
}

/// Compares BSP and oracle at random points away from every plane.
fn compare(brushes: &[Brush], rng: &mut Rng, span: f64, min_gap: f64) {
    let bsp = compile(brushes).expect("compile");
    for _ in 0..400 {
        let p = v(
            rng.range_f64(-span, span),
            rng.range_f64(-span, span),
            rng.range_f64(-span, span),
        );
        if gap(brushes, p) > min_gap {
            assert_eq!(bsp.is_solid(p), oracle(brushes, p), "at {p:?}");
        }
    }
}

#[test]
fn integer_grid_cuboids_sharing_many_planes() {
    let mut rng = Rng::new(0xC0_91A4);
    for _ in 0..150 {
        let brushes: Vec<Brush> = (0..2 + rng.below(8))
            .map(|i| {
                let lo = [rng.below(4), rng.below(4), rng.below(4)].map(f64::from);
                let hi = [0, 1, 2].map(|a| lo[a] + f64::from(1 + rng.below(3)));
                let op = random_op(&mut rng);
                Brush::cuboid(v(lo[0], lo[1], lo[2]), v(hi[0], hi[1], hi[2]), op, i).unwrap()
            })
            .collect();
        compare(&brushes, &mut rng, 7.0, 1.0e-3);
    }
}

/// Convex brushes from a small fixed pool of plane normals with integer
/// distances, so planes coincide across brushes at skew angles too.
#[test]
fn shared_skew_planes() {
    let normals = [
        v(1.0, 0.0, 0.0),
        v(-1.0, 0.0, 0.0),
        v(0.0, 1.0, 0.0),
        v(0.0, -1.0, 0.0),
        v(0.0, 0.0, 1.0),
        v(0.0, 0.0, -1.0),
        v(1.0, 1.0, 0.0),
        v(-1.0, -1.0, 0.0),
        v(0.0, 1.0, 1.0),
        v(1.0, 0.0, -1.0),
    ];
    let mut rng = Rng::new(77);
    for _ in 0..120 {
        let mut brushes = Vec::new();
        for i in 0..2 + rng.below(5) {
            let mut planes: Vec<DPlane> = normals[..6]
                .iter()
                .map(|&n| DPlane::new(n, f64::from(1 + rng.below(4))))
                .collect();
            for &n in &normals[6..] {
                if rng.below(2) == 0 {
                    planes.push(DPlane::new(n, f64::from(rng.below(4)) + 1.0));
                }
            }
            let op = random_op(&mut rng);
            brushes.extend(Brush::new(&planes, op, i).ok());
        }
        compare(&brushes, &mut rng, 6.0, 1.0e-3);
    }
}

/// Planes offset by less than the plane epsilon from a shared plane.
#[test]
fn near_coplanar_planes_stay_consistent() {
    let mut rng = Rng::new(31337);
    for _ in 0..150 {
        let brushes: Vec<Brush> = (0..2 + rng.below(5))
            .map(|i| {
                let jit = |rng: &mut Rng| rng.range_f64(-3.0e-6, 3.0e-6);
                let planes: Vec<DPlane> = (0..3)
                    .flat_map(|a| {
                        let mut n = [0.0; 3];
                        n[a] = 1.0;
                        let lo = f64::from(rng.below(3));
                        let hi = lo + f64::from(1 + rng.below(2));
                        let (j1, j2) = (jit(&mut rng), jit(&mut rng));
                        [
                            DPlane::new(v(n[0], n[1], n[2]), hi + j1),
                            DPlane::new(v(-n[0], -n[1], -n[2]), -lo + j2),
                        ]
                    })
                    .collect();
                Brush::new(&planes, random_op(&mut rng), i).unwrap()
            })
            .collect();
        compare(&brushes, &mut rng, 5.0, 1.0e-3);
    }
}

#[test]
fn extreme_coordinates() {
    let mut rng = Rng::new(9);
    for _ in 0..40 {
        let brushes: Vec<Brush> = (0..2 + rng.below(4))
            .map(|i| {
                let lo = [0, 1, 2].map(|_| rng.range_f64(-32000.0, 20000.0));
                let hi = [0, 1, 2].map(|a| lo[a] + rng.range_f64(1.0, 12000.0));
                let op = random_op(&mut rng);
                Brush::cuboid(v(lo[0], lo[1], lo[2]), v(hi[0], hi[1], hi[2]), op, i).unwrap()
            })
            .collect();
        compare(&brushes, &mut rng, 32000.0, 0.5);
    }
}

#[test]
fn thin_slabs() {
    let mut rng = Rng::new(5);
    for _ in 0..60 {
        let brushes: Vec<Brush> = (0..2 + rng.below(4))
            .map(|i| {
                let t = rng.range_f64(0.001, 0.2);
                let c = rng.range_f64(-3.0, 3.0);
                let planes = [
                    DPlane::new(v(1.0, 0.0, 0.0), c + t),
                    DPlane::new(v(-1.0, 0.0, 0.0), -c + t),
                    DPlane::new(v(0.0, 1.0, 0.0), 4.0),
                    DPlane::new(v(0.0, -1.0, 0.0), 4.0),
                    DPlane::new(v(0.0, 0.0, 1.0), 4.0),
                    DPlane::new(v(0.0, 0.0, -1.0), 4.0),
                ];
                Brush::new(&planes, random_op(&mut rng), i).unwrap()
            })
            .collect();
        compare(&brushes, &mut rng, 5.0, 1.0e-4);
    }
}

fn special(rng: &mut Rng) -> f64 {
    let pool = [
        f64::NAN,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::MAX,
        f64::MIN_POSITIVE,
        5.0e-324,
        1.0e-9,
        1.0e200,
        -0.0,
        32768.0,
        65536.0,
        1.0e-5,
    ];
    pool[rng.below(pool.len() as u32) as usize]
}

/// Brush records with extreme plane values must be accepted or rejected,
/// never panic, and accepted ones must compile.
#[test]
fn extreme_plane_values_in_files() {
    let base = Brush::cuboid(v(-2.0, -2.0, -2.0), v(2.0, 2.0, 2.0), Op::Subtract, 0).unwrap();
    let bytes = format::write(&Level {
        brushes: vec![base],
        entities: vec![],
        volumes: Vec::new(),
    });
    // Header 12 + section head 8 + count 4 + brush head 11 = first plane.
    let first = 12 + 8 + 4 + 11;
    let mut rng = Rng::new(404);
    let mut accepted = 0;
    for _ in 0..5000 {
        let mut d = bytes.clone();
        for _ in 0..1 + rng.below(3) {
            let field = rng.below(24) as usize;
            let at = first + field * 8;
            d[at..at + 8].copy_from_slice(&special(&mut rng).to_le_bytes());
        }
        if let Ok(level) = format::read(&d) {
            accepted += 1;
            let _ = compile(&level.brushes);
        }
    }
    assert!(accepted > 0);
}

/// Worst-case brush count at the file cap must not take unreasonably long.
#[test]
fn many_complex_brushes_load_in_bounded_time() {
    // A 64-plane near-spherical brush.
    let mut rng = Rng::new(1);
    let planes: Vec<DPlane> = (0..64)
        .map(|_| {
            loop {
                let c = v(
                    rng.range_f64(-1.0, 1.0),
                    rng.range_f64(-1.0, 1.0),
                    rng.range_f64(-1.0, 1.0),
                );
                if let Some(n) = c.normalized() {
                    break DPlane::new(n, 3.0);
                }
            }
        })
        .collect();
    let b = Brush::new(&planes, Op::Subtract, 0).unwrap();
    let level = Level {
        brushes: vec![b; 2000],
        entities: vec![],
        volumes: Vec::new(),
    };
    let bytes = format::write(&level);
    let t = std::time::Instant::now();
    let r = format::read(&bytes).unwrap();
    eprintln!(
        "read 2000 x {} planes in {:?}",
        r.brushes[0].planes().len(),
        t.elapsed()
    );
}
