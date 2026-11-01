//! A convex solid cell as geometry: faces, edges and vertices recovered from
//! the plane set accumulated while walking the BSP, plus the exact sweep of a
//! sphere against it (face, edge-cylinder and vertex-sphere contacts).

use crate::prim::{Contact, ray_segment_cylinder, ray_sphere};
use crate::tuning::{MIN_EDGE, MIN_FACE_AREA, ON_PLANE};
use mscent_math::Vec3d;
use mscent_world::polytope::MAX_COORD;

/// Half-space `normal . p <= dist` bounding a cell from outside.
#[derive(Clone, Copy, Debug)]
pub(crate) struct CellPlane {
    pub normal: Vec3d,
    pub dist: f64,
}

impl CellPlane {
    fn signed(&self, p: Vec3d) -> f64 {
        self.normal.dot(p) - self.dist
    }
}

/// Convex cell with its boundary features. Cells on the world's outer shell
/// are unbounded in the tree, so they are clipped to a box that no mover can
/// reach; the box faces are artificial and never produce contacts.
pub(crate) struct Cell {
    planes: Vec<CellPlane>,
    /// `has_face[i]`: real plane `i` bounds the cell with a non-degenerate face.
    has_face: Vec<bool>,
    real: usize,
    edges: Vec<(Vec3d, Vec3d)>,
    verts: Vec<Vec3d>,
}

fn box_planes() -> [CellPlane; 6] {
    let b = MAX_COORD * 2.0;
    let mk = |x, y, z| CellPlane {
        normal: Vec3d::new(x, y, z),
        dist: b,
    };
    [
        mk(1.0, 0.0, 0.0),
        mk(-1.0, 0.0, 0.0),
        mk(0.0, 1.0, 0.0),
        mk(0.0, -1.0, 0.0),
        mk(0.0, 0.0, 1.0),
        mk(0.0, 0.0, -1.0),
    ]
}

/// A huge square lying on `pl`, the seed that the other planes clip down.
fn seed_quad(pl: &CellPlane) -> Vec<Vec3d> {
    let n = pl.normal;
    let helper = if n.x.abs() < 0.6 {
        Vec3d::new(1.0, 0.0, 0.0)
    } else {
        Vec3d::new(0.0, 1.0, 0.0)
    };
    let u = n
        .cross(helper)
        .normalized()
        .unwrap_or(Vec3d::new(1.0, 0.0, 0.0));
    let w = n.cross(u);
    let c = n * pl.dist;
    let h = MAX_COORD * 8.0;
    vec![
        c + u * h + w * h,
        c - u * h + w * h,
        c - u * h - w * h,
        c + u * h - w * h,
    ]
}

/// Sutherland-Hodgman against the half-space `signed <= 0`.
fn clip(poly: &[Vec3d], pl: &CellPlane) -> Vec<Vec3d> {
    let mut out = Vec::with_capacity(poly.len() + 1);
    for (i, &a) in poly.iter().enumerate() {
        let b = poly[(i + 1) % poly.len()];
        let (da, db) = (pl.signed(a), pl.signed(b));
        if da <= 0.0 {
            out.push(a);
        }
        if (da < 0.0 && db > 0.0) || (da > 0.0 && db < 0.0) {
            out.push(a + (b - a) * (da / (da - db)));
        }
    }
    out
}

fn area(poly: &[Vec3d]) -> f64 {
    let mut sum = Vec3d::ZERO;
    for (i, &a) in poly.iter().enumerate() {
        sum = sum + a.cross(poly[(i + 1) % poly.len()]);
    }
    sum.length() * 0.5
}

fn face_polygon(i: usize, all: &[CellPlane]) -> Vec<Vec3d> {
    let mut poly = seed_quad(&all[i]);
    for (j, pl) in all.iter().enumerate() {
        if j != i && !poly.is_empty() {
            poly = clip(&poly, pl);
        }
    }
    if poly.len() >= 3 && area(&poly) >= MIN_FACE_AREA {
        poly
    } else {
        Vec::new()
    }
}

impl Cell {
    /// Builds the cell bounded by `planes` (all outward-facing).
    pub fn build(planes: &[CellPlane]) -> Self {
        let real = planes.len();
        let mut all = planes.to_vec();
        all.extend(box_planes());
        let polys: Vec<Vec<Vec3d>> = (0..real).map(|i| face_polygon(i, &all)).collect();
        let mut cell = Self {
            has_face: polys.iter().map(|p| !p.is_empty()).collect(),
            planes: all,
            real,
            edges: Vec::new(),
            verts: Vec::new(),
        };
        cell.collect_features(&polys);
        cell
    }

    fn near_box(&self, p: Vec3d) -> bool {
        self.planes[self.real..]
            .iter()
            .any(|b| b.signed(p) >= -ON_PLANE)
    }

    fn collect_features(&mut self, polys: &[Vec<Vec3d>]) {
        for (i, poly) in polys.iter().enumerate() {
            for (k, &p) in poly.iter().enumerate() {
                let q = poly[(k + 1) % poly.len()];
                if !self.near_box(p) && !self.near_box(q) && (q - p).length() >= MIN_EDGE {
                    self.add_edge(i, p, q);
                }
                self.add_vertex(p);
            }
        }
    }

    /// Keeps the edge once, from the lower-numbered of its two faces.
    fn add_edge(&mut self, i: usize, p: Vec3d, q: Vec3d) {
        let shared = (i + 1..self.real).any(|j| {
            self.has_face[j]
                && self.planes[j].signed(p).abs() <= ON_PLANE
                && self.planes[j].signed(q).abs() <= ON_PLANE
        });
        if shared {
            self.edges.push((p, q));
        }
    }

    fn add_vertex(&mut self, p: Vec3d) {
        if self.near_box(p) || self.verts.iter().any(|v| (*v - p).length() <= ON_PLANE) {
            return;
        }
        self.verts.push(p);
    }

    /// True if `p` is inside or on every plane except `skip`.
    fn contains_except(&self, p: Vec3d, skip: usize) -> bool {
        self.planes
            .iter()
            .enumerate()
            .all(|(j, pl)| j == skip || pl.signed(p) <= ON_PLANE)
    }

    /// First contact of a sphere of `radius` whose centre moves `from` to
    /// `from + d`, no later than `limit`, with this cell's boundary.
    pub fn sweep(&self, from: Vec3d, d: Vec3d, radius: f64, limit: f64) -> Option<Contact> {
        let mut best: Option<Contact> = None;
        let mut offer = |c: Option<Contact>| {
            if let Some(c) = c.filter(|c| c.t <= limit && best.is_none_or(|b| c.t < b.t)) {
                best = Some(c);
            }
        };
        for i in 0..self.real {
            offer(self.face_contact(i, from, d, radius, limit));
        }
        for &(a, b) in &self.edges {
            offer(ray_segment_cylinder(from, d, a, b, radius));
        }
        for &v in &self.verts {
            offer(ray_sphere(from, d, v, radius));
        }
        best
    }

    /// Plane `i` pushed out by the radius, valid only where the touching
    /// point lies on the face itself rather than on its extension.
    fn face_contact(&self, i: usize, from: Vec3d, d: Vec3d, r: f64, limit: f64) -> Option<Contact> {
        if !self.has_face[i] {
            return None;
        }
        let pl = &self.planes[i];
        let approach = pl.normal.dot(d);
        if approach >= 0.0 {
            return None;
        }
        let dist0 = pl.signed(from);
        if dist0 < -r {
            // Wholly behind the plane: the foot is out of reach, and a
            // contact here would be with a face the sphere already passed.
            return None;
        }
        let gap = dist0 - r;
        let t = if gap <= 0.0 { 0.0 } else { gap / -approach };
        if t > limit {
            return None;
        }
        let at = from + d * t;
        let foot = at - pl.normal * pl.signed(at);
        self.contains_except(foot, i).then_some(Contact {
            t,
            normal: pl.normal,
        })
    }
}
