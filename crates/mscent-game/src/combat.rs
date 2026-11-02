//! Rays and hitscan, shared by the weapon, the enemies and line of sight.
//!
//! Everything here goes through `mscent_collide::sweep_sphere`, the same
//! trace movement uses, so a shot can never pass through a wall the player
//! could not fly through.

use mscent_collide::{Trace, sweep_sphere, sweep_sphere_sphere};
use mscent_math::Vec3;
use mscent_world::Bsp;

/// A shootable sphere.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sphere {
    pub centre: Vec3,
    pub radius: f32,
}

/// Result of [`hitscan`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hitscan {
    /// Index into the target slice of the first sphere struck, if any.
    pub target: Option<usize>,
    /// Distance from the origin to the strike, a wall or the end of range.
    pub distance: f32,
    pub point: Vec3,
    /// Unit normal of what was struck; zero when nothing was.
    pub normal: Vec3,
}

/// Zero-radius trace from `from` to `to`.
pub fn ray(bsp: &Bsp, from: Vec3, to: Vec3) -> Trace {
    sweep_sphere(bsp, from, to, 0.0)
}

/// True when nothing solid lies between the two points.
pub fn line_of_sight(bsp: &Bsp, a: Vec3, b: Vec3) -> bool {
    let t = ray(bsp, a, b);
    !t.start_solid && !t.hit
}

/// Fires a ray of `range` from `origin` along `dir` and reports the nearest
/// of the walls and `targets`. A zero or non-finite direction misses.
pub fn hitscan(bsp: &Bsp, origin: Vec3, dir: Vec3, range: f32, targets: &[Sphere]) -> Hitscan {
    let miss = Hitscan {
        target: None,
        distance: 0.0,
        point: origin,
        normal: Vec3::ZERO,
    };
    let Some(dir) = dir
        .normalized()
        .filter(|_| range.is_finite() && range > 0.0)
    else {
        return miss;
    };
    let end = origin + dir * range;
    let wall = ray(bsp, origin, end);
    if wall.start_solid {
        return miss;
    }
    let wall_fraction = if wall.hit { wall.fraction } else { 1.0 };
    let mut best: Option<(usize, f32)> = None;
    for (i, s) in targets.iter().enumerate() {
        let hit = sweep_sphere_sphere(origin, end, 0.0, s.centre, s.radius);
        let near = hit.map(|h| h.fraction).filter(|&f| f < wall_fraction);
        if let Some(f) = near.filter(|&f| best.is_none_or(|(_, b)| f < b)) {
            best = Some((i, f));
        }
    }
    let fraction = best.map_or(wall_fraction, |(_, f)| f);
    let point = origin + dir * (fraction * range);
    let normal = match best {
        // A ray starting inside the sphere has no direction to report.
        Some((i, _)) => (point - targets[i].centre)
            .normalized()
            .unwrap_or(Vec3::ZERO),
        None if wall.hit => wall.normal,
        None => Vec3::ZERO,
    };
    Hitscan {
        target: best.map(|(i, _)| i),
        distance: fraction * range,
        point,
        normal,
    }
}
