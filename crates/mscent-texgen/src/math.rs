//! `sin`, `cos` and `pow` from series. The platform's versions round differently
//! between libraries, and one recipe must give one set of bytes everywhere.

const TAU: f32 = core::f32::consts::TAU;
const LN2: f32 = core::f32::consts::LN_2;

/// The part of `x` past the whole tile below it, `0.0..1.0`.
#[inline]
pub fn frac(x: f32) -> f32 {
    x - x.floor()
}

/// Signed distance along a tile whose edges meet: `d` folded into
/// `-0.5..0.5`, so a shape near one edge reaches texels at the other.
#[inline]
pub fn wrap_delta(d: f32) -> f32 {
    d - (d + 0.5).floor()
}

/// Sine and cosine of `angle` in radians. Finite for every input; a non-finite
/// angle reads as 0.
pub fn sin_cos(angle: f32) -> (f32, f32) {
    if !angle.is_finite() {
        return (0.0, 1.0);
    }
    let turns = angle / TAU;
    // Into half a turn either way, then folded to a quarter, where the series
    // is good.
    let x = (turns - (turns + 0.5).floor()) * TAU;
    let half_pi = TAU / 4.0;
    let (x, flip) = if x > half_pi {
        (TAU / 2.0 - x, -1.0)
    } else if x < -half_pi {
        (-TAU / 2.0 - x, -1.0)
    } else {
        (x, 1.0)
    };
    let x2 = x * x;
    let sin = x * (1.0 - x2 / 6.0 * (1.0 - x2 / 20.0 * (1.0 - x2 / 42.0 * (1.0 - x2 / 72.0))));
    let cos = 1.0 - x2 / 2.0 * (1.0 - x2 / 12.0 * (1.0 - x2 / 30.0 * (1.0 - x2 / 56.0)));
    (sin, cos * flip)
}

/// Natural logarithm of a positive finite number, from its exponent and a series
/// on the mantissa.
fn ln(x: f32) -> f32 {
    let bits = x.to_bits();
    let exponent = ((bits >> 23) & 0xff) as i32 - 127;
    let m = f32::from_bits((bits & 0x007f_ffff) | 0x3f80_0000);
    let t = (m - 1.0) / (m + 1.0);
    let t2 = t * t;
    let series = 2.0 * t * (1.0 + t2 * (1.0 / 3.0 + t2 * (0.2 + t2 * (1.0 / 7.0 + t2 / 9.0))));
    exponent as f32 * LN2 + series
}

/// `e^x`, by powers of two and a series on the rest. Saturates instead of
/// overflowing.
fn exp(x: f32) -> f32 {
    let k = (x / LN2).round().clamp(-126.0, 127.0);
    let r = x - k * LN2;
    let series = 1.0 + r * (1.0 + r / 2.0 * (1.0 + r / 3.0 * (1.0 + r / 4.0 * (1.0 + r / 5.0))));
    series * f32::from_bits(((k as i32 + 127) as u32) << 23)
}

/// `base^power` for `base` in `0.0..=1.0`, which is the use gamma has for it.
/// Bases outside that range clamp to it, and a NaN base reads as 0.
pub fn pow01(base: f32, power: f32) -> f32 {
    if base.is_nan() || base <= 0.0 {
        return 0.0;
    }
    if base >= 1.0 {
        return 1.0;
    }
    exp(ln(base) * power).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sin_cos_match_the_platform_closely() {
        let mut a = -40.0f32;
        while a < 40.0 {
            let (s, c) = sin_cos(a);
            assert!((s - a.sin()).abs() < 1e-3, "sin {a}: {s}");
            assert!((c - a.cos()).abs() < 1e-3, "cos {a}: {c}");
            a += 0.0137;
        }
    }

    #[test]
    fn sin_cos_survive_any_input() {
        assert_eq!(sin_cos(f32::NAN), (0.0, 1.0));
        assert_eq!(sin_cos(f32::INFINITY), (0.0, 1.0));
        let (s, c) = sin_cos(1e30);
        assert!(s.is_finite() && c.is_finite());
    }

    #[test]
    fn pow_matches_the_platform_closely() {
        for b in [0.001f32, 0.05, 0.2, 0.5, 0.75, 0.99] {
            for p in [0.1f32, 0.45, 1.0, 2.2, 8.0] {
                let got = pow01(b, p);
                assert!((got - b.powf(p)).abs() < 1e-3, "{b}^{p}: {got}");
            }
        }
    }

    #[test]
    fn wrapped_distances_fold_into_half_a_tile() {
        assert_eq!(wrap_delta(0.75), -0.25);
        assert_eq!(wrap_delta(-0.75), 0.25);
        assert_eq!(wrap_delta(3.0), 0.0);
        assert_eq!(wrap_delta(0.25), 0.25);
        assert_eq!(frac(-0.25), 0.75);
        assert_eq!(frac(2.5), 0.5);
        assert!(wrap_delta(1e30).abs() <= 0.5);
    }

    #[test]
    fn pow_clamps_its_edges() {
        assert_eq!(pow01(0.0, 2.0), 0.0);
        assert_eq!(pow01(1.0, 2.0), 1.0);
        assert_eq!(pow01(f32::NAN, 2.0), 0.0);
        assert_eq!(pow01(-3.0, 2.0), 0.0);
        assert_eq!(pow01(7.0, 2.0), 1.0);
        assert!(pow01(1e-30, 1e30) <= 1.0);
    }
}
