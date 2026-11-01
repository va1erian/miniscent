use crate::brush::{Brush, BrushError, MAX_PLANES, Op};
use mscent_math::rng::Rng;
use mscent_math::{DPlane, Polygon, Vec3d};

fn v(x: f64, y: f64, z: f64) -> Vec3d {
    Vec3d::new(x, y, z)
}

fn cube() -> Brush {
    Brush::cuboid(v(0.0, 0.0, 0.0), v(4.0, 4.0, 4.0), Op::Add, 0).unwrap()
}

#[test]
fn cuboid_has_six_faces_matching_planes() {
    let b = cube();
    assert_eq!(b.planes().len(), 6);
    assert_eq!(b.faces().len(), 6);
    for (plane, face) in b.planes().iter().zip(b.faces()) {
        assert!((face.area() - 16.0).abs() < 1e-9);
        let n = face.normal().unwrap();
        assert!(n.dot(plane.normal) > 0.999_999);
        for &p in face.vertices() {
            assert!(plane.signed_distance(p).abs() < 1e-9);
        }
    }
    let bounds = b.bounds();
    assert_eq!(bounds.min.x, 0.0);
    assert_eq!(bounds.max.z, 4.0);
}

#[test]
fn contains_and_flip() {
    let b = cube();
    assert!(b.contains(v(2.0, 2.0, 2.0)));
    assert!(!b.contains(v(5.0, 2.0, 2.0)));
    assert_eq!(b.flipped_op().op(), Op::Subtract);
    assert_eq!(b.order(), 0);
}

#[test]
fn cuboid_snaps_to_grid() {
    let b = Brush::cuboid(v(0.01, 0.0, 0.0), v(3.99, 4.0, 4.0), Op::Add, 0).unwrap();
    assert_eq!(b.bounds().min.x, 0.0);
    assert_eq!(b.bounds().max.x, 4.0);
}

#[test]
fn rejects_bad_cuboids() {
    let o = v(0.0, 0.0, 0.0);
    let bad = |a, b| Brush::cuboid(a, b, Op::Add, 0).unwrap_err();
    assert_eq!(
        bad(v(1.0, 0.0, 0.0), v(0.0, 1.0, 1.0)),
        BrushError::Degenerate
    );
    assert_eq!(bad(o, v(0.0, 1.0, 1.0)), BrushError::Degenerate);
    assert_eq!(bad(o, v(f64::NAN, 1.0, 1.0)), BrushError::NotFinite);
    assert_eq!(bad(o, v(1.0, f64::INFINITY, 1.0)), BrushError::NotFinite);
    assert_eq!(bad(o, v(1.0e6, 1.0, 1.0)), BrushError::OutOfRange);
}

#[test]
fn rejects_bad_plane_sets() {
    let axis = |x, y, z, d| DPlane::new(v(x, y, z), d);
    let new = |p: &[DPlane]| Brush::new(p, Op::Add, 0).unwrap_err();
    assert_eq!(new(&[]), BrushError::TooFewPlanes);
    let c = cube();
    assert_eq!(new(&c.planes()[..3]), BrushError::TooFewPlanes);
    // Only five planes: open at the top.
    assert_eq!(new(&c.planes()[..5]), BrushError::Unbounded);
    // Inverted: opposite normals leave nothing.
    let inverted: Vec<DPlane> = c.planes().iter().map(DPlane::flipped).collect();
    assert_eq!(new(&inverted), BrushError::Empty);
    // Zero normal and NaN.
    let mut zero = c.planes().to_vec();
    zero[0] = axis(0.0, 0.0, 0.0, 1.0);
    assert_eq!(new(&zero), BrushError::Degenerate);
    zero[0] = axis(f64::NAN, 0.0, 0.0, 1.0);
    assert_eq!(new(&zero), BrushError::NotFinite);
    zero[0] = axis(1.0, 0.0, 0.0, f64::INFINITY);
    assert_eq!(new(&zero), BrushError::NotFinite);
    // Too many.
    let many = vec![c.planes()[0]; MAX_PLANES + 1];
    assert_eq!(new(&many), BrushError::TooManyPlanes);
    // Flat slab.
    let flat = [
        axis(1.0, 0.0, 0.0, 1.0),
        axis(-1.0, 0.0, 0.0, -1.0),
        axis(0.0, 1.0, 0.0, 1.0),
        axis(0.0, -1.0, 0.0, 1.0),
        axis(0.0, 0.0, 1.0, 1.0),
        axis(0.0, 0.0, -1.0, 1.0),
    ];
    assert_eq!(new(&flat), BrushError::Empty);
}

#[test]
fn redundant_and_duplicate_planes_are_dropped() {
    let c = cube();
    let mut planes = c.planes().to_vec();
    planes.push(DPlane::new(v(1.0, 0.0, 0.0), 100.0));
    planes.push(c.planes()[2]);
    planes.push(DPlane::new(v(0.0, 2.0, 0.0), 8.0)); // unnormalised duplicate
    let b = Brush::new(&planes, Op::Subtract, 3).unwrap();
    assert_eq!(b.planes().len(), 6);
}

#[test]
fn non_unit_normals_are_normalised() {
    let planes = [
        DPlane::new(v(2.0, 0.0, 0.0), 8.0),
        DPlane::new(v(-3.0, 0.0, 0.0), 0.0),
        DPlane::new(v(0.0, 5.0, 0.0), 20.0),
        DPlane::new(v(0.0, -5.0, 0.0), 0.0),
        DPlane::new(v(0.0, 0.0, 7.0), 28.0),
        DPlane::new(v(0.0, 0.0, -7.0), 0.0),
    ];
    let b = Brush::new(&planes, Op::Add, 0).unwrap();
    assert!(b.contains(v(2.0, 2.0, 2.0)) && !b.contains(v(4.5, 2.0, 2.0)));
}

#[test]
fn point_triples_build_a_tetrahedron() {
    let (a, b, c, d) = (
        v(0.0, 0.0, 0.0),
        v(4.0, 0.0, 0.0),
        v(0.0, 4.0, 0.0),
        v(0.0, 0.0, 4.0),
    );
    // Outward winding for each face.
    let t = [[a, c, b], [a, b, d], [a, d, c], [b, c, d]];
    let br = Brush::from_point_triples(&t, Op::Add, 0).unwrap();
    assert_eq!(br.faces().len(), 4);
    assert!(br.contains(v(0.5, 0.5, 0.5)) && !br.contains(v(3.0, 3.0, 3.0)));
    let collinear = [[a, a, b]; 4];
    assert_eq!(
        Brush::from_point_triples(&collinear, Op::Add, 0).unwrap_err(),
        BrushError::Degenerate
    );
}

#[test]
fn from_polygons_accepts_cube_rejects_notch() {
    let faces: Vec<Polygon> = cube().faces().to_vec();
    assert!(Brush::from_polygons(&faces, Op::Add, 0).is_ok());
    // A dented cube: push one face's plane inward past its neighbours' span
    // by replacing the top face with one that sits below part of the walls.
    let mut dented = faces.clone();
    let shift = v(0.0, 0.0, -1.0);
    let moved: Vec<Vec3d> = dented[4].vertices().iter().map(|&p| p + shift).collect();
    // Tilt a corner outward so the face is in front of a neighbour's plane.
    let mut tilted = moved;
    tilted[0] = tilted[0] + v(-2.0, 0.0, 0.0);
    dented[4] = Polygon::new(tilted).unwrap();
    assert!(Brush::from_polygons(&dented, Op::Add, 0).is_err());
    // A concave (arrow-shaped) polygon face.
    let arrow = Polygon::new(vec![
        v(0.0, 0.0, 0.0),
        v(4.0, 0.0, 0.0),
        v(1.0, 1.0, 0.0),
        v(0.0, 4.0, 0.0),
    ])
    .unwrap();
    let mut with_arrow = faces;
    with_arrow[0] = arrow;
    assert_eq!(
        Brush::from_polygons(&with_arrow, Op::Add, 0).unwrap_err(),
        BrushError::NonConvex
    );
}

fn junk(rng: &mut Rng) -> f64 {
    match rng.below(8) {
        0 => f64::NAN,
        1 => f64::INFINITY,
        2 => f64::NEG_INFINITY,
        3 => 0.0,
        4 => f64::MAX,
        5 => f64::MIN_POSITIVE,
        _ => rng.range_f64(-100.0, 100.0),
    }
}

#[test]
fn fuzz_random_planes_never_panic_and_accepted_brushes_are_sane() {
    let mut rng = Rng::new(0xB2057);
    let mut accepted = 0;
    for _ in 0..3000 {
        let n = 3 + rng.below(8) as usize;
        let planes: Vec<DPlane> = (0..n)
            .map(|_| {
                let nv = v(junk(&mut rng), junk(&mut rng), junk(&mut rng));
                DPlane::new(nv, junk(&mut rng))
            })
            .collect();
        if let Ok(b) = Brush::new(&planes, Op::Add, 0) {
            accepted += 1;
            assert!(b.faces().len() >= 4);
            let bb = b.bounds();
            assert!(!bb.is_empty());
            assert!(bb.max.x.abs() <= 32769.0 && bb.min.x.abs() <= 32769.0);
        }
    }
    // Pure junk is almost never a valid brush; the point is no panic.
    assert!(accepted < 3000);
}

#[test]
fn fuzz_random_finite_planes_accepted_brushes_contain_their_vertices() {
    let mut rng = Rng::new(77);
    let mut accepted = 0;
    for _ in 0..500 {
        let n = 4 + rng.below(6) as usize;
        let planes: Vec<DPlane> = (0..n)
            .map(|_| {
                let nv = v(
                    rng.range_f64(-1.0, 1.0),
                    rng.range_f64(-1.0, 1.0),
                    rng.range_f64(-1.0, 1.0),
                );
                DPlane::new(nv, rng.range_f64(1.0, 20.0))
            })
            .collect();
        if let Ok(b) = Brush::new(&planes, Op::Subtract, 1) {
            accepted += 1;
            for f in b.faces() {
                for &p in f.vertices() {
                    assert!(b.planes().iter().all(|pl| pl.signed_distance(p) < 1e-6));
                }
            }
        }
    }
    assert!(accepted > 50, "only {accepted} accepted");
}
