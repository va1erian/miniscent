//! Where a generator grows: a point on a surface, which way the surface faces,
//! and how big the thing should be.

use mscent_math::Vec3d;

use crate::model::pick::Ray;

/// Smallest and largest size a generator accepts, in world units. The size is
/// the radius of the tool, which the panel limits the same way.
pub const MIN_SIZE: f64 = 0.25;
pub const MAX_SIZE: f64 = 8.0;

/// A place to grow something.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Site {
    /// A point on the surface.
    pub origin: Vec3d,
    /// Which way the surface faces: out of the rock, into the air.
    pub normal: Vec3d,
    /// Scale of the result: a tree is about six of these tall.
    pub size: f64,
    /// The cell of the volume it is built in.
    pub cell: f64,
    /// Material of the main part.
    pub material: u8,
}

impl Site {
    /// The local frame, or `None` for a site that cannot be built on: no normal,
    /// or an origin or size out of range.
    pub fn frame(&self) -> Option<Frame> {
        let sized = (MIN_SIZE..=MAX_SIZE).contains(&self.size) && self.cell > 0.0;
        if !(sized && self.origin.is_finite()) {
            return None;
        }
        Frame::new(self.origin, self.normal)
    }
}

/// Axes at a site: `n` out of the surface, `u` and `v` along it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Frame {
    pub origin: Vec3d,
    pub n: Vec3d,
    pub u: Vec3d,
    pub v: Vec3d,
}

impl Frame {
    pub fn new(origin: Vec3d, normal: Vec3d) -> Option<Self> {
        let n = normal.normalized()?;
        // Whichever world axis is least along `n` gives a stable first tangent.
        let helper = if n.z.abs() < 0.9 {
            Vec3d::new(0.0, 0.0, 1.0)
        } else {
            Vec3d::new(1.0, 0.0, 0.0)
        };
        let u = n.cross(helper).normalized()?;
        let v = n.cross(u);
        Some(Self { origin, n, u, v })
    }

    /// The point `x` along `u`, `y` along `v` and `h` out of the surface.
    pub fn at(&self, x: f64, y: f64, h: f64) -> Vec3d {
        self.origin + self.u * x + self.v * y + self.n * h
    }

    /// A direction made of the same parts.
    pub fn dir(&self, x: f64, y: f64, h: f64) -> Vec3d {
        self.u * x + self.v * y + self.n * h
    }
}

/// The surface normal at the end of `ray`, from where three parallel rays
/// `spread` apart land. `hit` gives the distance a ray travels before it meets
/// the world. A flat or curved surface gives its normal; a hit on an edge or a
/// miss falls back to facing the ray, which is always valid.
pub fn surface_normal(ray: &Ray, spread: f64, mut hit: impl FnMut(&Ray) -> Option<f64>) -> Vec3d {
    let back = ray.dir * -1.0;
    let Some(dir) = ray.dir.normalized() else {
        return back;
    };
    let side = Frame::new(ray.origin, dir).map_or(Vec3d::ZERO, |f| f.u);
    let up = Frame::new(ray.origin, dir).map_or(Vec3d::ZERO, |f| f.v);
    let mut point = |offset: Vec3d| {
        let r = Ray {
            origin: ray.origin + offset,
            dir,
        };
        hit(&r).map(|t| r.origin + dir * t)
    };
    let tilt = (|| {
        let p0 = point(Vec3d::ZERO)?;
        let p1 = point(side * spread)?;
        let p2 = point(up * spread)?;
        let n = (p1 - p0).cross(p2 - p0).normalized()?;
        // Facing the viewer: the side of the surface the ray came from.
        Some(if n.dot(dir) > 0.0 { n * -1.0 } else { n })
    })();
    tilt.unwrap_or(back)
}
