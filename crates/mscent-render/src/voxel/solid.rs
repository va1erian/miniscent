//! Spots that belong to the volume a material fills, not to a picture laid on it.
//!
//! A surface of any shape cuts a ball in a round patch, whichever way it faces, so
//! nothing here can be stretched or cut where texture projections meet. That is
//! what a large, bright mark on a curved surface needs, and a projected image
//! cannot give it. The table holds distance to the nearest ball rather than whether
//! a sample is in one, so its edge stays clean however close the eye comes. One
//! fetch reads eight samples, and only for the pixels of a material that has spots.

use std::sync::OnceLock;

use mscent_math::Vec3;
use mscent_math::rng::Rng;
use mscent_world::materials::CAP;

/// Samples along each edge of the table, which repeats. A power of two.
const EDGE: usize = 32;
/// World units between samples.
const SPACING: f32 = 0.1;
/// Distance at which the table saturates, in world units.
const BAND: f32 = 0.4;
/// Balls tried, seed and sizes. Most fit; those too near another are dropped, so
/// spots never run together.
const TRIES: u32 = 600;
const SEED: u64 = 0x5907;
const RADII: (f64, f64) = (0.17, 0.27);
const MIN_GAP: f32 = 0.16;
/// The softest an edge gets, in world units, so that it never looks like a stair.
const MIN_EDGE: f32 = 0.012;
/// World size of a pixel at which single spots are lost, and the mean takes over.
const LOST_AT: f32 = 0.2;
/// The colour of a spot.
pub(super) const SPOT_COLOR: u32 = 0xffed_ddb3;

/// A repeating table of distance to the nearest ball.
pub(super) struct Spots {
    distance: Vec<i8>,
    /// Share of the volume inside a ball, which is what a far surface shows.
    mean: f32,
}

#[inline]
const fn index(x: i32, y: i32, z: i32) -> usize {
    let m = EDGE as i32 - 1;
    (x & m) as usize + ((y & m) as usize) * EDGE + ((z & m) as usize) * EDGE * EDGE
}

impl Spots {
    fn new() -> Self {
        let mut rng = Rng::new(SEED);
        let size = EDGE as f64 * f64::from(SPACING);
        let mut balls: Vec<(Vec3, f32)> = Vec::new();
        for _ in 0..TRIES {
            let mut at = || rng.range_f64(0.0, size) as f32;
            let c = Vec3::new(at(), at(), at());
            let r = rng.range_f64(RADII.0, RADII.1) as f32;
            let clear = balls.iter().all(|&(o, q)| {
                let wrap = |d: f32| {
                    let s = size as f32;
                    (d - s * (d / s).round()).abs()
                };
                let (dx, dy, dz) = (wrap(c.x - o.x), wrap(c.y - o.y), wrap(c.z - o.z));
                (dx * dx + dy * dy + dz * dz).sqrt() >= r + q + MIN_GAP
            });
            if clear {
                balls.push((c, r));
            }
        }
        let mut distance = vec![i8::MAX; EDGE * EDGE * EDGE];
        for &(c, r) in &balls {
            mark_ball(&mut distance, c, r);
        }
        let inside = distance.iter().filter(|&&d| d < 0).count();
        Self {
            mean: inside as f32 / distance.len() as f32,
            distance,
        }
    }

    /// How much of the pixel at `p` is spot, for a pixel `pixel` world units wide.
    #[inline]
    pub(super) fn coverage(&self, p: Vec3, pixel: f32) -> f32 {
        let inv = 1.0 / SPACING;
        let (fx, fy, fz) = (p.x * inv, p.y * inv, p.z * inv);
        let (bx, by, bz) = (fx.floor(), fy.floor(), fz.floor());
        let (x, y, z) = (bx as i32, by as i32, bz as i32);
        let (tx, ty, tz) = (fx - bx, fy - by, fz - bz);
        let at = |dx: i32, dy: i32, dz: i32| {
            let i = index(x.wrapping_add(dx), y.wrapping_add(dy), z.wrapping_add(dz));
            f32::from(self.distance[i])
        };
        let along_x = |dy, dz| at(0, dy, dz) + (at(1, dy, dz) - at(0, dy, dz)) * tx;
        let near = along_x(0, 0) + (along_x(1, 0) - along_x(0, 0)) * ty;
        let far = along_x(0, 1) + (along_x(1, 1) - along_x(0, 1)) * ty;
        let d = (near + (far - near) * tz) * BAND / f32::from(i8::MAX);
        let edge = (0.5 - d / pixel.max(MIN_EDGE)).clamp(0.0, 1.0);
        // Only as a spot nears a pixel across: nearer, it must keep its full colour.
        edge + (self.mean - edge) * (2.0 * pixel / LOST_AT - 1.0).clamp(0.0, 1.0)
    }
}

/// Keeps in `table` the distance to the ball at `c`, wherever it is the nearest.
fn mark_ball(table: &mut [i8], c: Vec3, r: f32) {
    let reach = ((r + BAND) / SPACING).ceil() as i32;
    let (cx, cy, cz) = (
        (c.x / SPACING).round() as i32,
        (c.y / SPACING).round() as i32,
        (c.z / SPACING).round() as i32,
    );
    for z in cz - reach..=cz + reach {
        for y in cy - reach..=cy + reach {
            for x in cx - reach..=cx + reach {
                let at = Vec3::new(x as f32, y as f32, z as f32) * SPACING;
                let d = ((at - c).length() - r) / BAND;
                let value = (d.clamp(-1.0, 1.0) * f32::from(i8::MAX)) as i8;
                let slot = &mut table[index(x, y, z)];
                *slot = (*slot).min(value);
            }
        }
    }
}

/// Whether `material` has spots, and so must take the path that draws them.
#[inline]
pub(super) fn has_spots(material: u8) -> bool {
    u16::from(material) == CAP
}

/// The spots of `material`, if it has any.
pub(super) fn spots_of(material: u8) -> Option<&'static Spots> {
    static SPOTS: OnceLock<Spots> = OnceLock::new();
    has_spots(material).then(|| SPOTS.get_or_init(Spots::new))
}

#[cfg(test)]
mod tests {
    use super::*;

    const CAP_ID: u8 = CAP as u8;

    fn spots() -> &'static Spots {
        spots_of(CAP_ID).expect("a cap has spots")
    }

    #[test]
    fn only_the_cap_has_spots() {
        assert!(spots_of(CAP_ID).is_some());
        assert!(spots_of(0).is_none());
        assert!(spots_of(u8::MAX).is_none());
    }

    #[test]
    fn spots_cover_a_share_that_reads_as_spots() {
        let mean = spots().mean;
        assert!((0.05..0.25).contains(&mean), "{mean}");
    }

    #[test]
    fn the_table_repeats_with_no_seam() {
        let size = EDGE as f32 * SPACING;
        let (s, mut worst) = (spots(), 0.0f32);
        for i in 0..2000 {
            let f = i as f32 * 0.0137;
            let p = Vec3::new(f.sin() * 9.0, f * 0.3, (f * 0.7).cos() * 9.0);
            let shifted = p + Vec3::new(size, -size * 2.0, size * 3.0);
            worst = worst.max((s.coverage(p, 0.05) - s.coverage(shifted, 0.05)).abs());
        }
        assert!(worst < 0.05, "{worst}");
    }

    /// Any point of a straight line through the table, coming and going by whole
    /// spots: an edge is crossed, not a jump to some other value.
    #[test]
    fn coverage_changes_smoothly_along_a_line() {
        let (s, mut worst) = (spots(), 0.0f32);
        let mut last = s.coverage(Vec3::ZERO, 0.05);
        for i in 1..20_000 {
            let t = i as f32 * 0.001;
            let now = s.coverage(Vec3::new(t, t * 0.61, t * 0.37), 0.05);
            worst = worst.max((now - last).abs());
            last = now;
        }
        assert!(worst < 0.1, "{worst}");
    }

    #[test]
    fn a_far_surface_shows_the_mean_not_a_pattern() {
        let s = spots();
        for i in 0..200 {
            let p = Vec3::new(i as f32 * 0.07, i as f32 * 0.03, 1.0);
            assert!((s.coverage(p, LOST_AT * 2.0) - s.mean).abs() < 1e-3);
        }
    }

    #[test]
    fn no_position_can_panic_or_break_the_range() {
        let s = spots();
        for p in [
            Vec3::new(f32::NAN, 0.0, 0.0),
            Vec3::new(f32::INFINITY, f32::NEG_INFINITY, 0.0),
            Vec3::new(-1.0e9, 1.0e9, 3.0),
        ] {
            let c = s.coverage(p, 0.05);
            assert!(c.is_nan() || (0.0..=1.0).contains(&c), "{c}");
        }
    }
}
