//! Face extraction for convex regions given as plane sets. Brush validation
//! and the CSG merge both need "is this intersection of half-spaces
//! non-empty, and what are its faces", so the one routine lives here.

use mscent_math::{DPlane, Polygon, Split, Vec3d, epsilon};

/// Largest coordinate magnitude a brush may use. Keeps `f64` arithmetic far
/// from precision trouble and `f32` runtime data exact enough.
pub const MAX_COORD: f64 = 32768.0;

/// Half-size of the seed quad. Well above [`MAX_COORD`] so a vertex that
/// survives clipping near this size identifies an unbounded region.
pub const SEED_HALF: f64 = 4.0 * MAX_COORD;

/// A very large quad lying on `plane`, wound counter-clockwise seen from the
/// front. `None` only if the normal is not usable.
fn seed_quad(plane: &DPlane) -> Option<Polygon> {
    let n = plane.normal;
    // Pick the world axis least aligned with the normal for a stable basis.
    let axis = if n.x.abs() <= n.y.abs() && n.x.abs() <= n.z.abs() {
        Vec3d::new(1.0, 0.0, 0.0)
    } else if n.y.abs() <= n.z.abs() {
        Vec3d::new(0.0, 1.0, 0.0)
    } else {
        Vec3d::new(0.0, 0.0, 1.0)
    };
    let u = n.cross(axis).normalized()? * SEED_HALF;
    let v = n.cross(u).normalized()? * SEED_HALF;
    let c = n * plane.dist;
    Polygon::new(vec![c - u - v, c + u - v, c + u + v, c - u + v])
}

/// Clips the seed polygon of `planes[i]` by every other plane, keeping what is
/// behind them. `None` when nothing of the face remains.
fn clip_face(planes: &[DPlane], i: usize) -> Option<Polygon> {
    let mut poly = seed_quad(&planes[i])?;
    for (j, other) in planes.iter().enumerate() {
        if j == i {
            continue;
        }
        poly = match poly.split_by_plane(other) {
            Split::Back => poly,
            Split::Spanning { back, .. } => back,
            Split::Front => return None,
            // Coplanar: the same orientation is a duplicate (keep the first
            // copy only); the opposite orientation squeezes the region flat.
            Split::On => {
                if planes[i].normal.dot(other.normal) > 0.0 && i < j {
                    poly
                } else {
                    return None;
                }
            }
        };
    }
    (poly.area() >= epsilon::MIN_POLYGON_AREA).then_some(poly)
}

/// Faces of the convex region `{ p : n_i . p <= d_i }` as `(plane index,
/// polygon)` pairs, polygons wound counter-clockwise seen from outside.
/// Redundant planes produce no face. Unbounded regions yield polygons with
/// vertices near `SEED_HALF`; callers check with [`max_abs_coord`].
pub fn faces(planes: &[DPlane]) -> Vec<(usize, Polygon)> {
    (0..planes.len())
        .filter_map(|i| clip_face(planes, i).map(|p| (i, p)))
        .collect()
}

/// True if the region has positive volume (at least a tetrahedron of faces).
pub fn has_volume(planes: &[DPlane]) -> bool {
    faces(planes).len() >= 4
}

/// Largest absolute coordinate over all polygon vertices.
pub fn max_abs_coord<'a>(polys: impl IntoIterator<Item = &'a Polygon>) -> f64 {
    polys
        .into_iter()
        .flat_map(|p| p.vertices().iter())
        .map(|v| v.x.abs().max(v.y.abs()).max(v.z.abs()))
        .fold(0.0, f64::max)
}
