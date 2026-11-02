//! Fields for the Fill tool: a formula over the whole box, positive where it is
//! solid, in world units, so the surface sits where the formula is zero.

use mscent_math::Vec3d;
use mscent_math::rng::Rng;
use mscent_world::voxel::noise3;

use std::f64::consts::TAU;

/// Wavelengths the panel may ask for, in world units.
pub const MIN_WAVELENGTH: f64 = 2.0;
pub const MAX_WAVELENGTH: f64 = 64.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FieldKind {
    /// A warped gyroid: smooth, endlessly connected passages with no dead ends.
    Gyroid,
    /// Ridged noise: winding tunnels like a worn cave system.
    Noise,
}

impl FieldKind {
    pub const ALL: [FieldKind; 2] = [FieldKind::Gyroid, FieldKind::Noise];
    pub const NAMES: [&'static str; 2] = ["Warped gyroid", "Cave noise"];
}

/// The field for `kind`, with one passage about `wavelength` across and
/// details from `seed`. Hostile wavelengths are brought into range.
pub fn field(kind: FieldKind, seed: u32, wavelength: f64) -> impl Fn(Vec3d) -> f64 {
    let wavelength = if wavelength.is_finite() {
        wavelength.clamp(MIN_WAVELENGTH, MAX_WAVELENGTH)
    } else {
        MIN_WAVELENGTH
    };
    let k = TAU / wavelength;
    let mut rng = Rng::new(u64::from(seed));
    let phase = [0, 1, 2].map(|_| rng.range_f64(0.0, TAU));
    move |p| match kind {
        FieldKind::Gyroid => gyroid(p, k, phase),
        FieldKind::Noise => ridged(p, k, seed),
    }
}

/// The gyroid is warped by two finer ones, so passages bulge and pinch. Dividing
/// by `k` makes its slope about one, as a distance should be.
fn gyroid(p: Vec3d, k: f64, phase: [f64; 3]) -> f64 {
    let g = |x: f64, y: f64, z: f64| x.sin() * y.cos() + y.sin() * z.cos() + z.sin() * x.cos();
    let at = |scale: f64, shift: [f64; 3]| {
        g(
            p.x * k * scale + shift[0],
            p.y * k * scale + shift[1],
            p.z * k * scale + shift[2],
        )
    };
    let sum = at(1.0, phase) + at(3.1, [3.0, 0.0, 1.0]) * 0.18 + at(7.4, [0.0, 2.0, 0.0]) * 0.05;
    (sum + 0.25) * 0.7 / k
}

/// Air where noise crosses zero, solid elsewhere: a ridge of the noise is a tunnel.
fn ridged(p: Vec3d, k: f64, seed: u32) -> f64 {
    let q = p * (k * 0.5);
    let n = noise3(seed, q) * 0.7 + noise3(seed.wrapping_add(1), q * 2.3) * 0.3;
    (n.abs() - 0.12) / k * 2.0
}
