//! Soft shadows: a lamp is a small disc facing the surface point, and the
//! shadow ray count that reaches it gives the visibility. The penumbra a
//! disc makes is several luxels wide, so shadow edges are smooth ramps in
//! the baked data instead of one-luxel steps (which alias when seen from a
//! distance).

use super::ray::segment_clear;
use mscent_math::Vec3;

/// Samples tested first: the centre and three on the rim. If they agree the
/// point is fully lit or fully shadowed and the rest are skipped, which is
/// the case for nearly every luxel.
const PROBE: u32 = 4;

/// Golden angle, for an even spread of sample points over the disc.
const GOLDEN: f32 = 2.399_963_2;

/// Fraction of a lamp of `radius` at `light` visible from `origin`
/// (`origin` is already off the surface). `samples` rays at most; `bsp` is
/// queried through `clear`. `spin` rotates the pattern per receiver so the
/// remaining sampling error is fine noise, not stripes.
pub(super) fn visibility(
    clear: &dyn Fn(Vec3) -> bool,
    origin: Vec3,
    light: Vec3,
    radius: f32,
    samples: u32,
    spin: f32,
) -> f32 {
    let samples = samples.max(1);
    if radius <= 0.0 || samples == 1 {
        return f32::from(u8::from(clear(light)));
    }
    let Some(axis) = (light - origin).normalized() else {
        return 0.0;
    };
    let side = if axis.x.abs() < 0.9 {
        Vec3::new(1.0, 0.0, 0.0)
    } else {
        Vec3::new(0.0, 1.0, 0.0)
    };
    let Some(a) = axis.cross(side).normalized() else {
        return 0.0;
    };
    let b = axis.cross(a);
    let point = |k: u32| {
        // k = 0 is the centre; the others spiral outwards.
        let r = radius * ((k as f32 + 0.5) / samples as f32).sqrt() * f32::from(u8::from(k > 0));
        let t = spin + k as f32 * GOLDEN;
        light + a * (r * t.cos()) + b * (r * t.sin())
    };
    // Probes: the centre and the last PROBE - 1 samples (rim of the spiral).
    let probe = PROBE.min(samples);
    let is_probe = |k: u32| k == 0 || k + probe > samples;
    let hits = (0..samples)
        .filter(|&k| is_probe(k) && clear(point(k)))
        .count() as u32;
    if hits == 0 || hits == probe {
        return hits as f32 / probe as f32;
    }
    let rest = (0..samples)
        .filter(|&k| !is_probe(k) && clear(point(k)))
        .count() as u32;
    (hits + rest) as f32 / samples as f32
}

/// Pattern rotation for a receiver at `p`: a hash of its position, so that
/// neighbouring luxels use different rotations but a rebake gives the same
/// result.
pub(super) fn spin_of(p: Vec3) -> f32 {
    let h = p.x.to_bits().wrapping_mul(0x9E37_79B1)
        ^ p.y.to_bits().wrapping_mul(0x85EB_CA6B)
        ^ p.z.to_bits().wrapping_mul(0xC2B2_AE35);
    let h = (h ^ (h >> 15)).wrapping_mul(0x2C1B_3C6D);
    (h >> 8) as f32 * (core::f32::consts::TAU / 16_777_216.0)
}

/// `segment_clear` from `origin` to `target` as a closure input.
pub(super) fn clear_to(bsp: &crate::bsp::Bsp, origin: Vec3) -> impl Fn(Vec3) -> bool + '_ {
    let from = origin.to_f64();
    move |target| segment_clear(bsp, from, target.to_f64())
}
