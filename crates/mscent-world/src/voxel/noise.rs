//! Seeded 3D value noise: the same seed and point always give the same number,
//! on any machine, so roughening and generated fields replay exactly.

use mscent_math::Vec3d;

/// A hash of a lattice corner and a seed, in `-1..=1`. An integer mix keeps it
/// free of platform float differences.
fn corner(seed: u32, x: i64, y: i64, z: i64) -> f64 {
    let mut h = u64::from(seed) ^ 0x9E37_79B9_7F4A_7C15;
    for v in [x, y, z] {
        h = (h ^ v as u64).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        h ^= h >> 31;
    }
    h = h.wrapping_mul(0x94D0_49BB_1331_11EB);
    h ^= h >> 29;
    (h >> 11) as f64 / (1u64 << 52) as f64 - 1.0
}

/// Smooth interpolation weight with zero slope at both ends.
fn fade(t: f64) -> f64 {
    t * t * (3.0 - 2.0 * t)
}

/// Value noise in `-1..=1`, with features about one unit across. Points too far
/// out to index the lattice give zero.
pub fn noise3(seed: u32, p: Vec3d) -> f64 {
    const LIMIT: f64 = 1.0e9;
    if ![p.x, p.y, p.z].iter().all(|v| v.abs() < LIMIT) {
        return 0.0;
    }
    let base = [p.x.floor(), p.y.floor(), p.z.floor()];
    let w = [p.x - base[0], p.y - base[1], p.z - base[2]].map(fade);
    let at = base.map(|b| b as i64);
    let lerp = |a: f64, b: f64, t: f64| a + (b - a) * t;
    let mut layer = [0.0; 2];
    for (dz, slot) in layer.iter_mut().enumerate() {
        let row = |dy: i64| {
            let a = corner(seed, at[0], at[1] + dy, at[2] + dz as i64);
            let b = corner(seed, at[0] + 1, at[1] + dy, at[2] + dz as i64);
            lerp(a, b, w[0])
        };
        *slot = lerp(row(0), row(1), w[1]);
    }
    lerp(layer[0], layer[1], w[2])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noise_repeats_stays_in_range_and_follows_the_seed() {
        let mut differs = false;
        for i in 0..500 {
            let p = Vec3d::new(f64::from(i) * 0.37, f64::from(i) * 0.11 - 9.0, 2.5);
            let n = noise3(7, p);
            assert!((-1.0..=1.0).contains(&n), "{n}");
            assert_eq!(n, noise3(7, p));
            differs |= n != noise3(8, p);
        }
        assert!(differs, "another seed gives another field");
    }

    #[test]
    fn noise_is_smooth_and_survives_hostile_points() {
        let p = Vec3d::new(1.3, 2.2, 0.4);
        let near = Vec3d::new(1.3001, 2.2, 0.4);
        assert!((noise3(1, p) - noise3(1, near)).abs() < 0.01);
        assert_eq!(noise3(1, Vec3d::new(f64::NAN, 0.0, 0.0)), 0.0);
        assert_eq!(noise3(1, Vec3d::new(1.0e300, 0.0, 0.0)), 0.0);
    }
}
