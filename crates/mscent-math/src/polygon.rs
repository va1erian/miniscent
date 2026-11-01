//! Convex polygons in `f64`, with splitting by a plane. World building and
//! the renderer's clipper both need this primitive.

use crate::epsilon;
use crate::plane::{DPlane, PolySide, Side};
use crate::vec::Vec3d;

/// A planar polygon with at least three finite vertices. Splitting assumes it
/// is convex; the type does not check that, callers feed it convex input.
#[derive(Clone, Debug, PartialEq)]
pub struct Polygon {
    verts: Vec<Vec3d>,
}

/// Outcome of splitting a polygon by a plane.
#[derive(Clone, Debug, PartialEq)]
pub enum Split {
    /// Entirely in front (within epsilon); the caller keeps the original.
    Front,
    /// Entirely behind (within epsilon); the caller keeps the original.
    Back,
    /// Lies in the plane (within epsilon); the caller decides by orientation.
    On,
    /// Cut in two. Vertices on the plane belong to both pieces.
    Spanning { front: Polygon, back: Polygon },
}

impl Polygon {
    /// `None` if there are fewer than three vertices or any is not finite.
    pub fn new(verts: Vec<Vec3d>) -> Option<Self> {
        if verts.len() < 3 || !verts.iter().all(|v| v.is_finite()) {
            return None;
        }
        Some(Self { verts })
    }

    pub fn vertices(&self) -> &[Vec3d] {
        &self.verts
    }

    /// Vector area (Newell's method), robust to collinear leading vertices.
    fn area_vector(&self) -> Vec3d {
        let mut sum = Vec3d::ZERO;
        let mut prev = self.verts[self.verts.len() - 1];
        for &v in &self.verts {
            sum = sum + prev.cross(v);
            prev = v;
        }
        sum * 0.5
    }

    pub fn area(&self) -> f64 {
        self.area_vector().length()
    }

    /// Unit normal following the vertex winding; `None` for a zero-area
    /// polygon.
    pub fn normal(&self) -> Option<Vec3d> {
        self.area_vector().normalized()
    }

    /// The supporting plane, `None` for a zero-area polygon.
    pub fn plane(&self) -> Option<DPlane> {
        let n = self.normal()?;
        Some(DPlane::new(n, n.dot(self.verts[0])))
    }

    /// Splits by `plane`. A piece thinner than [`epsilon::MIN_POLYGON_AREA`] is
    /// dropped, so a plane that merely grazes a polygon reports `Front` or
    /// `Back` instead of producing a sliver.
    pub fn split_by_plane(&self, plane: &DPlane) -> Split {
        let dists: Vec<f64> = self
            .verts
            .iter()
            .map(|&v| plane.signed_distance(v))
            .collect();
        let sides: Vec<Side> = self
            .verts
            .iter()
            .map(|&v| plane.classify_point(v))
            .collect();
        match PolySide::from_sides(sides.iter().copied()) {
            PolySide::Front => Split::Front,
            PolySide::Back => Split::Back,
            PolySide::On => Split::On,
            PolySide::Spanning => self.cut(&dists, &sides),
        }
    }

    fn cut(&self, dists: &[f64], sides: &[Side]) -> Split {
        let n = self.verts.len();
        let (mut front, mut back) = (Vec::with_capacity(n + 2), Vec::with_capacity(n + 2));
        for i in 0..n {
            let j = (i + 1) % n;
            let (a, b) = (self.verts[i], self.verts[j]);
            match sides[i] {
                Side::Front => front.push(a),
                Side::Back => back.push(a),
                Side::On => {
                    front.push(a);
                    back.push(a);
                }
            }
            if matches!(
                (sides[i], sides[j]),
                (Side::Front, Side::Back) | (Side::Back, Side::Front)
            ) {
                // Opposite signs guarantee a nonzero denominator.
                let t = dists[i] / (dists[i] - dists[j]);
                let hit = a.lerp(b, t);
                front.push(hit);
                back.push(hit);
            }
        }
        match (Self::sliver_free(front), Self::sliver_free(back)) {
            (Some(front), Some(back)) => Split::Spanning { front, back },
            (Some(_), None) => Split::Front,
            (None, Some(_)) => Split::Back,
            (None, None) => Split::On,
        }
    }

    fn sliver_free(verts: Vec<Vec3d>) -> Option<Self> {
        let p = Self::new(verts)?;
        (p.area() >= epsilon::MIN_POLYGON_AREA).then_some(p)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rng::Rng;

    fn v(x: f64, y: f64, z: f64) -> Vec3d {
        Vec3d::new(x, y, z)
    }

    fn square() -> Polygon {
        Polygon::new(vec![
            v(0.0, 0.0, 0.0),
            v(2.0, 0.0, 0.0),
            v(2.0, 2.0, 0.0),
            v(0.0, 2.0, 0.0),
        ])
        .unwrap()
    }

    fn x_plane(d: f64) -> DPlane {
        DPlane::new(v(1.0, 0.0, 0.0), d)
    }

    #[test]
    fn rejects_bad_input() {
        assert!(Polygon::new(vec![v(0.0, 0.0, 0.0), v(1.0, 0.0, 0.0)]).is_none());
        let nan = vec![v(0.0, 0.0, 0.0), v(1.0, 0.0, 0.0), v(f64::NAN, 1.0, 0.0)];
        assert!(Polygon::new(nan).is_none());
    }

    #[test]
    fn area_normal_plane() {
        let s = square();
        assert!((s.area() - 4.0).abs() < 1e-12);
        assert_eq!(s.normal().unwrap(), v(0.0, 0.0, 1.0));
        assert_eq!(s.plane().unwrap().dist, 0.0);
        let line =
            Polygon::new(vec![v(0.0, 0.0, 0.0), v(1.0, 0.0, 0.0), v(2.0, 0.0, 0.0)]).unwrap();
        assert!(line.normal().is_none());
    }

    #[test]
    fn whole_polygon_cases() {
        let s = square();
        assert_eq!(s.split_by_plane(&x_plane(-1.0)), Split::Front);
        assert_eq!(s.split_by_plane(&x_plane(3.0)), Split::Back);
        assert_eq!(
            s.split_by_plane(&DPlane::new(v(0.0, 0.0, 1.0), 0.0)),
            Split::On
        );
        // Touching along an edge does not split.
        assert_eq!(s.split_by_plane(&x_plane(0.0)), Split::Front);
        assert_eq!(s.split_by_plane(&x_plane(2.0)), Split::Back);
    }

    #[test]
    fn midline_split_halves_the_square() {
        let Split::Spanning { front, back } = square().split_by_plane(&x_plane(0.5)) else {
            panic!("expected a spanning split");
        };
        assert!((front.area() - 3.0).abs() < 1e-12);
        assert!((back.area() - 1.0).abs() < 1e-12);
        assert_eq!(front.vertices().len(), 4);
    }

    #[test]
    fn diagonal_through_vertices_makes_two_triangles() {
        let diag = DPlane::new(v(1.0, -1.0, 0.0).normalized().unwrap(), 0.0);
        let Split::Spanning { front, back } = square().split_by_plane(&diag) else {
            panic!("expected a spanning split");
        };
        assert_eq!(front.vertices().len(), 3);
        assert_eq!(back.vertices().len(), 3);
        assert!((front.area() - 2.0).abs() < 1e-12);
        assert!((back.area() - 2.0).abs() < 1e-12);
    }

    #[test]
    fn plane_just_inside_the_edge_band_does_not_sliver() {
        let eps = epsilon::PLANE_EPSILON_F64;
        // The cut sits 2*eps from the edge: a real but sub-area-threshold piece.
        let split = square().split_by_plane(&x_plane(2.0 * eps));
        match split {
            Split::Spanning { front, back } => {
                assert!(front.area() >= epsilon::MIN_POLYGON_AREA);
                assert!(back.area() >= epsilon::MIN_POLYGON_AREA);
            }
            Split::Front => {}
            other => panic!("unexpected {other:?}"),
        }
    }

    /// A random convex polygon: points on an ellipse, in angular order, in a
    /// randomly oriented plane.
    fn random_convex(rng: &mut Rng) -> Option<Polygon> {
        let n = rng.below(7) as usize + 3;
        let normal = random_unit(rng);
        let helper = if normal.x.abs() < 0.9 {
            v(1.0, 0.0, 0.0)
        } else {
            v(0.0, 1.0, 0.0)
        };
        let u = normal.cross(helper).normalized()?;
        let w = normal.cross(u);
        let (ru, rw) = (rng.range_f64(0.5, 6.0), rng.range_f64(0.5, 6.0));
        let center = v(
            rng.range_f64(-5.0, 5.0),
            rng.range_f64(-5.0, 5.0),
            rng.range_f64(-5.0, 5.0),
        );
        let mut angles: Vec<f64> = (0..n)
            .map(|_| rng.range_f64(0.0, core::f64::consts::TAU))
            .collect();
        angles.sort_by(f64::total_cmp);
        let verts = angles
            .iter()
            .map(|a| center + u * (ru * a.cos()) + w * (rw * a.sin()))
            .collect();
        let p = Polygon::new(verts)?;
        (p.area() > 1e-2).then_some(p)
    }

    fn random_unit(rng: &mut Rng) -> Vec3d {
        loop {
            let c = v(
                rng.range_f64(-1.0, 1.0),
                rng.range_f64(-1.0, 1.0),
                rng.range_f64(-1.0, 1.0),
            );
            if let Some(n) = c.normalized() {
                return n;
            }
        }
    }

    fn is_convex_with(p: &Polygon, normal: Vec3d) -> bool {
        let vs = p.vertices();
        let n = vs.len();
        (0..n).all(|i| {
            let (a, b, c) = (vs[i], vs[(i + 1) % n], vs[(i + 2) % n]);
            (b - a).cross(c - b).dot(normal) >= -1e-9
        })
    }

    #[test]
    fn property_split_pieces_are_on_their_sides_and_areas_sum() {
        let mut rng = Rng::new(0xC5C5_0001);
        let (mut spanning, mut on_vertex) = (0, 0);
        for case in 0..5000 {
            let Some(poly) = random_convex(&mut rng) else {
                continue;
            };
            let mut plane = DPlane::new(random_unit(&mut rng), 0.0);
            // Aim near the polygon, and sometimes exactly through a vertex.
            let target = if rng.below(5) == 0 {
                on_vertex += 1;
                poly.vertices()[rng.below(poly.vertices().len() as u32) as usize]
            } else {
                poly.vertices()[0].lerp(poly.vertices()[1], rng.next_f64())
                    + poly.vertices()[2].lerp(poly.vertices()[0], rng.next_f64()) * 0.1
            };
            plane.dist = plane.normal.dot(target);

            if let Split::Spanning { front, back } = poly.split_by_plane(&plane) {
                spanning += 1;
                let eps = epsilon::PLANE_EPSILON_F64;
                for p in front.vertices() {
                    assert!(
                        plane.signed_distance(*p) >= -eps,
                        "case {case}: front vertex behind"
                    );
                }
                for p in back.vertices() {
                    assert!(
                        plane.signed_distance(*p) <= eps,
                        "case {case}: back vertex in front"
                    );
                }
                let total = front.area() + back.area();
                assert!(
                    (total - poly.area()).abs() <= 1e-9 * poly.area().max(1.0),
                    "case {case}: area {} vs {}",
                    total,
                    poly.area()
                );
                let n = poly.normal().unwrap();
                assert!(is_convex_with(&front, n), "case {case}: front not convex");
                assert!(is_convex_with(&back, n), "case {case}: back not convex");
                // Winding (and so the facing direction) is preserved.
                for piece in [&front, &back] {
                    if piece.area() > 1e-6 {
                        assert!(
                            piece.normal().unwrap().dot(n) > 0.999_999,
                            "case {case}: flipped"
                        );
                    }
                }
            }
        }
        // Guard against a vacuous test: most cases must actually split.
        assert!(spanning > 1500, "only {spanning} spanning cases");
        assert!(on_vertex > 500);
    }

    #[test]
    fn property_non_spanning_polygons_are_wholly_on_one_side() {
        let mut rng = Rng::new(0xC5C5_0002);
        for _ in 0..2000 {
            let Some(poly) = random_convex(&mut rng) else {
                continue;
            };
            let plane = DPlane::new(random_unit(&mut rng), rng.range_f64(-12.0, 12.0));
            // Dropped slivers may leave a vertex a little past epsilon, so
            // allow a loose margin; the exact bound is checked above.
            let eps = 1.0e-3;
            let ds: Vec<f64> = poly
                .vertices()
                .iter()
                .map(|&p| plane.signed_distance(p))
                .collect();
            match poly.split_by_plane(&plane) {
                Split::Front => assert!(ds.iter().all(|&d| d >= -eps)),
                Split::Back => assert!(ds.iter().all(|&d| d <= eps)),
                Split::On => assert!(ds.iter().all(|&d| d.abs() <= eps)),
                Split::Spanning { .. } => {}
            }
        }
    }
}
