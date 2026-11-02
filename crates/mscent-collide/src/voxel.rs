//! Sweeping a sphere through the triangles of the voxel surfaces.
//!
//! The contact model is that of a solid cell in `cell.rs`, applied to the
//! triangles that are drawn: each face pushed out by the radius (valid where
//! the touching point lies on the triangle), each edge a cylinder and each
//! vertex a sphere. A sphere rolling from a brush floor onto voxel rock
//! therefore meets the same kind of surface on both sides and no lip.
//!
//! Triangles are one-sided like the faces of a cell: the air side is where the
//! winding points, and a face only stops a sphere moving against that normal.
//! A sphere may therefore always leave a surface it is embedded in, which is
//! what keeps it from being trapped.

use crate::prim::{Contact, ray_segment_cylinder, ray_sphere};
use crate::tuning::{ON_PLANE, PRUNE_MARGIN};
use mscent_math::{Vec3, Vec3d};
use mscent_world::voxel::VoxelWorld;

/// First contact of a sphere of `radius` whose centre moves `from` to
/// `from + d`, no later than `limit`, with the voxel surfaces of `world`.
pub(crate) fn sweep(
    world: &VoxelWorld,
    from: Vec3d,
    d: Vec3d,
    radius: f64,
    limit: f64,
) -> Option<Contact> {
    let to = from + d;
    let reach = radius + PRUNE_MARGIN;
    let pad = Vec3d::new(reach, reach, reach);
    let (lo, hi) = ((from.min(to) - pad).to_f32(), (from.max(to) + pad).to_f32());
    let mut sweep = Sweep {
        from,
        d,
        radius,
        limit,
        best: None,
        lo: Vec3d::ZERO,
        hi: Vec3d::ZERO,
    };
    sweep.shrink_box();
    world.for_each_chunk_in(lo, hi, |chunk| {
        chunk.triangles_in(lo, hi, |tri| sweep.triangle(tri.map(|v| v.pos)));
    });
    sweep.best
}

struct Sweep {
    from: Vec3d,
    d: Vec3d,
    radius: f64,
    /// Contacts later than this are of no interest; it falls as better ones
    /// are found.
    limit: f64,
    best: Option<Contact>,
    /// The box around the part of the path still of interest, a radius out.
    lo: Vec3d,
    hi: Vec3d,
}

impl Sweep {
    fn triangle(&mut self, corners: [Vec3; 3]) {
        let [a, b, c] = corners.map(Vec3::to_f64);
        if !self.reaches(a.min(b).min(c), a.max(b).max(c)) {
            return;
        }
        let face = self.face(a, b, c);
        self.offer(face);
        for (p, q) in [(a, b), (b, c), (c, a)] {
            self.offer(ray_segment_cylinder(self.from, self.d, p, q, self.radius));
        }
        for p in [a, b, c] {
            self.offer(ray_sphere(self.from, self.d, p, self.radius));
        }
    }

    fn offer(&mut self, found: Option<Contact>) {
        if let Some(c) = found.filter(|c| c.t <= self.limit) {
            self.limit = c.t;
            self.best = Some(c);
            self.shrink_box();
        }
    }

    fn shrink_box(&mut self) {
        let end = self.from + self.d * self.limit;
        let reach = self.radius + PRUNE_MARGIN;
        let pad = Vec3d::new(reach, reach, reach);
        self.lo = self.from.min(end) - pad;
        self.hi = self.from.max(end) + pad;
    }

    /// False if a triangle with this bounding box cannot touch the sphere
    /// anywhere on the path of interest. Most of the triangles in the cells a
    /// sweep touches fail this, and it is far cheaper than their contacts.
    fn reaches(&self, lo: Vec3d, hi: Vec3d) -> bool {
        lo.x <= self.hi.x
            && hi.x >= self.lo.x
            && lo.y <= self.hi.y
            && hi.y >= self.lo.y
            && lo.z <= self.hi.z
            && hi.z >= self.lo.z
    }

    /// The triangle's plane pushed out by the radius, valid only where the
    /// touching point lies on the triangle itself.
    fn face(&self, a: Vec3d, b: Vec3d, c: Vec3d) -> Option<Contact> {
        let n = (b - a).cross(c - a).normalized()?;
        let approach = n.dot(self.d);
        if approach >= 0.0 {
            return None;
        }
        let plane = n.dot(a);
        let dist0 = n.dot(self.from) - plane;
        if dist0 < -self.radius {
            // Wholly behind the plane: in the rock, and free to leave it.
            return None;
        }
        let gap = dist0 - self.radius;
        let t = if gap <= 0.0 { 0.0 } else { gap / -approach };
        if t > self.limit {
            return None;
        }
        let at = self.from + self.d * t;
        let foot = at - n * (n.dot(at) - plane);
        on_triangle(foot, [a, b, c], n).then_some(Contact { t, normal: n })
    }
}

/// True if `foot`, which lies in the plane of normal `n`, is inside the
/// triangle or no further than `ON_PLANE` outside an edge.
fn on_triangle(foot: Vec3d, corners: [Vec3d; 3], n: Vec3d) -> bool {
    (0..3).all(|i| {
        let (p, q) = (corners[i], corners[(i + 1) % 3]);
        let e = q - p;
        let len = e.length();
        len > 0.0 && e.cross(foot - p).dot(n) >= -ON_PLANE * len
    })
}
