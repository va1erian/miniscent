use super::*;
use mscent_math::rng::Rng;

fn frustum() -> Frustum {
    // 90 degree square view, near 1, far 100.
    Frustum::new(1.0, 1.0, 1.0, 100.0)
}

fn v(x: f32, y: f32, z: f32) -> ClipVertex {
    ClipVertex {
        pos: Vec3::new(x, y, z),
        uv: Vec2::new(x, y),
    }
}

#[test]
fn polygon_fully_inside_is_unchanged() {
    let poly = [v(-1.0, -1.0, -5.0), v(1.0, -1.0, -5.0), v(0.0, 1.0, -5.0)];
    assert_eq!(clip_polygon(&frustum(), &poly), poly.to_vec());
}

#[test]
fn polygon_fully_outside_is_dropped() {
    let behind = [v(0.0, 0.0, 2.0), v(1.0, 0.0, 2.0), v(0.0, 1.0, 2.0)];
    assert!(clip_polygon(&frustum(), &behind).is_empty());
    let right = [v(50.0, 0.0, -5.0), v(51.0, 0.0, -5.0), v(50.0, 1.0, -5.0)];
    assert!(clip_polygon(&frustum(), &right).is_empty());
    let far = [
        v(0.0, 0.0, -500.0),
        v(1.0, 0.0, -500.0),
        v(0.0, 1.0, -500.0),
    ];
    assert!(clip_polygon(&frustum(), &far).is_empty());
}

#[test]
fn near_plane_crossing_is_cut_at_the_plane() {
    // A quad running from z = +4 (behind the eye) to z = -4.
    let quad = [
        v(-0.5, 0.0, 4.0),
        v(0.5, 0.0, 4.0),
        v(0.5, 0.0, -4.0),
        v(-0.5, 0.0, -4.0),
    ];
    let out = clip_polygon(&frustum(), &quad);
    assert_eq!(out.len(), 4);
    assert!(out.iter().all(|c| c.pos.z <= -1.0 + 1e-5));
    let nearest = out.iter().map(|c| c.pos.z).fold(f32::MIN, f32::max);
    assert!((nearest + 1.0).abs() < 1e-5);
    // uv is carried through the cut: it equals x, which is unchanged here.
    assert!(out.iter().all(|c| (c.uv.x - c.pos.x).abs() < 1e-5));
}

#[test]
fn side_plane_clip_interpolates_uv() {
    // Triangle poking out the right side at depth 4 (right edge at x = 4).
    let tri = [v(0.0, 0.0, -4.0), v(8.0, 0.0, -4.0), v(0.0, 1.0, -4.0)];
    let out = clip_polygon(&frustum(), &tri);
    assert!(out.len() >= 3);
    for c in &out {
        assert!(c.pos.x <= 4.0 + 1e-4);
        assert!((c.uv.x - c.pos.x).abs() < 1e-4, "uv must follow position");
    }
    assert!(out.iter().any(|c| (c.pos.x - 4.0).abs() < 1e-4));
}

#[test]
fn rejects_degenerate_and_non_finite_input() {
    let f = frustum();
    assert!(clip_polygon(&f, &[]).is_empty());
    assert!(clip_polygon(&f, &[v(0.0, 0.0, -5.0), v(1.0, 0.0, -5.0)]).is_empty());
    let nan = [v(f32::NAN, 0.0, -5.0), v(1.0, 0.0, -5.0), v(0.0, 1.0, -5.0)];
    assert!(clip_polygon(&f, &nan).is_empty());
    let big = vec![v(0.0, 0.0, -5.0); MAX_POLYGON_VERTICES + 1];
    assert!(clip_polygon(&f, &big).is_empty());
}

#[test]
fn random_triangles_clip_inside_the_frustum() {
    let f = frustum();
    let mut rng = Rng::new(3);
    let mut kept = 0;
    for _ in 0..2000 {
        let mut p = || {
            v(
                rng.range_f32(-30.0, 30.0),
                rng.range_f32(-30.0, 30.0),
                rng.range_f32(-120.0, 20.0),
            )
        };
        let tri = [p(), p(), p()];
        let out = clip_polygon(&f, &tri);
        kept += usize::from(!out.is_empty());
        // Tolerance scales with magnitude: interpolation is f32.
        assert!(
            out.iter()
                .all(|c| (0..6).all(|i| f.distance(i, c.pos) >= -1e-2))
        );
    }
    assert!(kept > 100, "test should exercise clipping, kept {kept}");
}
