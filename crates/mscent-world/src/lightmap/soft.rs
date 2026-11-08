//! Soft shadows, integrated over both ends of the ray: the lamp is a small
//! disc and the receiver is the whole luxel footprint, not one point. Each
//! sample pairs a point on the lamp (Vogel spiral, evenly spread) with a
//! point in the luxel (R2 sequence, evenly spread), both rotated per luxel
//! by a stable hash so the leftover error is fine noise, not stripes. The
//! result is an area-averaged visibility, so a shadow edge crossing a luxel
//! gives it a proportional partial value instead of a hard 0 or 1; that is
//! what keeps lightmap shadow borders from turning into a staircase.

use super::occluders::Occluders;
use super::uv::FaceProjection;
use super::{BakeConfig, PointLight};
use mscent_math::Vec3;

/// Samples tested first: the lamp centre and three on its rim. If they agree
/// the luxel is fully lit or fully shadowed and the rest are skipped, which
/// is the case for nearly every luxel.
const PROBE: u32 = 4;

/// Golden angle, for an even spread of sample points over the disc.
const GOLDEN: f32 = 2.399_963_2;

/// Plastic-number constants of the R2 low-discrepancy sequence.
const R2: [f32; 2] = [0.754_877_7, 0.569_840_3];

/// The patch of surface a receiver stands for, which the samples spread over.
pub(super) enum Footprint<'a> {
    /// A luxel of a face.
    Luxel {
        proj: &'a FaceProjection,
        verts: &'a [Vec3],
        /// Luxel column and row.
        x: u32,
        y: u32,
    },
    /// A voxel cell: a square of side `size` facing `normal`.
    Cell {
        normal: Vec3,
        tangent: Vec3,
        bitangent: Vec3,
        size: f32,
    },
}

impl Footprint<'_> {
    pub(super) fn normal(&self) -> Vec3 {
        match self {
            Footprint::Luxel { proj, .. } => proj.normal,
            Footprint::Cell { normal, .. } => *normal,
        }
    }

    /// The point at fractions `(a, b)` of the footprint around `centre`.
    fn point(&self, centre: Vec3, a: f32, b: f32) -> Vec3 {
        match self {
            Footprint::Luxel { proj, verts, x, y } => {
                proj.luxel_sample_at(verts, *x as f32 + a, *y as f32 + b)
            }
            Footprint::Cell {
                tangent,
                bitangent,
                size,
                ..
            } => centre + *tangent * ((a - 0.5) * size) + *bitangent * ((b - 0.5) * size),
        }
    }
}

/// The surface point being shaded.
pub(super) struct Receiver<'a> {
    pub occ: &'a Occluders<'a>,
    pub foot: Footprint<'a>,
    /// Distance rays start off the surface.
    pub bias: f32,
    /// The footprint's centre, which seeds the per-receiver rotation.
    pub centre: Vec3,
}

/// Light arriving at the receiver from one lamp and the unit direction towards
/// it, or `None` when out of range, facing away or shadowed.
pub(super) fn light_at(
    rx: &Receiver,
    light: &PointLight,
    cfg: &BakeConfig,
) -> Option<(Vec3, Vec3)> {
    let to = light.position - rx.centre;
    let dist = to.length();
    if dist >= light.radius {
        return None;
    }
    let dir = to.normalized()?;
    let facing = dir.dot(rx.foot.normal());
    if facing <= 0.0 {
        return None;
    }
    // Past the cone nothing arrives, so no ray is worth casting.
    let beam = match &light.cone {
        Some(cone) => {
            let from_light = -dir;
            if cone.misses(from_light) {
                return None;
            }
            cone.share(from_light)
        }
        None => 1.0,
    };
    let seen = visibility(rx, light.position, cfg.light_radius, cfg.shadow_samples);
    if seen <= 0.0 {
        return None;
    }
    let falloff = 1.0 - dist / light.radius;
    Some((
        light.color * (falloff * falloff * falloff * facing * seen * beam),
        dir,
    ))
}

/// Fraction of a lamp of `radius` at `light` visible from the luxel.
/// `samples` rays at most; 1 (or a zero radius) is one hard ray from the
/// luxel centre.
pub(super) fn visibility(r: &Receiver, light: Vec3, radius: f32, samples: u32) -> f32 {
    let samples = samples.max(1);
    let normal = r.foot.normal();
    let ray = |from: Vec3, to: Vec3| {
        r.occ
            .segment_clear((from + normal * r.bias).to_f64(), to.to_f64())
    };
    if radius <= 0.0 || samples == 1 {
        return f32::from(u8::from(ray(r.centre, light)));
    }
    let Some(axis) = (light - r.centre).normalized() else {
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
    let (spin, shift) = rotation_of(r.centre);
    let lamp_at = |k: u32| {
        // k = 0 is the centre; the others spiral outwards.
        let rad = radius * ((k as f32 + 0.5) / samples as f32).sqrt() * f32::from(u8::from(k > 0));
        let t = spin + k as f32 * GOLDEN;
        light + a * (rad * t.cos()) + b * (rad * t.sin())
    };
    let sees = |j: u32| {
        let k = lamp_index(j, samples);
        let fx = (shift[0] + j as f32 * R2[0]).fract();
        let fy = (shift[1] + j as f32 * R2[1]).fract();
        ray(r.foot.point(r.centre, fx, fy), lamp_at(k))
    };
    let probe = PROBE.min(samples);
    let hits = (0..probe).filter(|&j| sees(j)).count() as u32;
    if hits == 0 || hits == probe {
        return hits as f32 / probe as f32;
    }
    let rest = (probe..samples).filter(|&j| sees(j)).count() as u32;
    (hits + rest) as f32 / samples as f32
}

/// Lamp-disc index used by sample `j`: the first samples are the centre and
/// the rim end of the spiral, so the early-out probes span the lamp.
fn lamp_index(j: u32, samples: u32) -> u32 {
    match j {
        0 => 0,
        1..=3 => samples.saturating_sub(j),
        _ => j - 3,
    }
}

/// Per-luxel pattern rotation (lamp angle, luxel offsets) from a hash of the
/// luxel position, so neighbouring luxels differ but a rebake gives the same
/// result.
fn rotation_of(p: Vec3) -> (f32, [f32; 2]) {
    let spin = spin_of(p);
    let u = spin / core::f32::consts::TAU;
    (spin, [u, (u * 7.0).fract()])
}

fn spin_of(p: Vec3) -> f32 {
    let h = p.x.to_bits().wrapping_mul(0x9E37_79B1)
        ^ p.y.to_bits().wrapping_mul(0x85EB_CA6B)
        ^ p.z.to_bits().wrapping_mul(0xC2B2_AE35);
    let h = (h ^ (h >> 15)).wrapping_mul(0x2C1B_3C6D);
    (h >> 8) as f32 * (core::f32::consts::TAU / 16_777_216.0)
}
