//! Sphere against sphere sweeps for entities (M4.3), solved analytically in
//! the frame of the target so a moving target needs no special casing.

use crate::prim::ray_sphere;
use mscent_math::{Vec3, Vec3d};

/// First touch between two spheres.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SphereHit {
    /// Fraction of the move at which the spheres touch, `0.0..=1.0`.
    pub fraction: f32,
    /// Unit normal from the target's centre toward the mover's centre.
    pub normal: Vec3,
}

/// Mover of radius `radius` goes `from` to `to` past a fixed sphere.
pub fn sweep_sphere_sphere(
    from: Vec3,
    to: Vec3,
    radius: f32,
    centre: Vec3,
    other_radius: f32,
) -> Option<SphereHit> {
    sweep_spheres(from, to, radius, centre, centre, other_radius)
}

/// Both spheres move linearly over the same step. Spheres that already
/// overlap hit at fraction 0 only if they are closing on each other, so
/// separating or resting spheres never report a hit.
pub fn sweep_spheres(
    a_from: Vec3,
    a_to: Vec3,
    a_radius: f32,
    b_from: Vec3,
    b_to: Vec3,
    b_radius: f32,
) -> Option<SphereHit> {
    let (r1, r2) = (f64::from(a_radius), f64::from(b_radius));
    let finite = a_from.is_finite() && a_to.is_finite() && b_from.is_finite() && b_to.is_finite();
    if !finite || !(r1.is_finite() && r2.is_finite() && r1 >= 0.0 && r2 >= 0.0) {
        return None;
    }
    let rel_from = a_from.to_f64() - b_from.to_f64();
    let rel_d = (a_to.to_f64() - a_from.to_f64()) - (b_to.to_f64() - b_from.to_f64());
    let c = ray_sphere(rel_from, rel_d, Vec3d::ZERO, r1 + r2)?;
    Some(SphereHit {
        fraction: c.t as f32,
        normal: c.normal.to_f32(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(x: f32, y: f32, z: f32) -> Vec3 {
        Vec3::new(x, y, z)
    }

    const O: Vec3 = Vec3::ZERO;

    #[test]
    fn head_on_touches_at_sum_of_radii() {
        let h = sweep_sphere_sphere(O, v(10.0, 0.0, 0.0), 1.0, v(5.0, 0.0, 0.0), 1.0).unwrap();
        assert!((h.fraction - 0.3).abs() < 1e-6);
        assert!((h.normal.x + 1.0).abs() < 1e-6);
    }

    #[test]
    fn grazing_touches_just_inside_misses_just_outside() {
        let go = |y: f32| sweep_sphere_sphere(O, v(10.0, 0.0, 0.0), 1.0, v(5.0, y, 0.0), 1.0);
        let graze = go(2.0).unwrap();
        assert!((graze.fraction - 0.5).abs() < 1e-6);
        assert!(go(2.001).is_none());
        assert!(go(1.99).is_some());
    }

    #[test]
    fn at_rest_and_separating() {
        let at_rest = |gap: f32| sweep_sphere_sphere(O, O, 1.0, v(gap, 0.0, 0.0), 1.0);
        assert!(at_rest(2.0).is_none());
        assert!(at_rest(5.0).is_none());
        assert!(
            at_rest(1.0).is_none(),
            "overlap without motion is not a new hit"
        );
        let away = sweep_sphere_sphere(O, v(-3.0, 0.0, 0.0), 1.0, v(1.5, 0.0, 0.0), 1.0);
        assert!(away.is_none());
        let into = sweep_sphere_sphere(O, v(3.0, 0.0, 0.0), 1.0, v(1.5, 0.0, 0.0), 1.0);
        assert_eq!(into.unwrap().fraction, 0.0);
    }

    #[test]
    fn moving_target_uses_relative_motion() {
        // Closing speed is 10 per step, gap 8: touch at 0.8.
        let h = sweep_spheres(
            O,
            v(5.0, 0.0, 0.0),
            1.0,
            v(10.0, 0.0, 0.0),
            v(5.0, 0.0, 0.0),
            1.0,
        )
        .unwrap();
        assert!((h.fraction - 0.8).abs() < 1e-6);
    }

    #[test]
    fn non_finite_input_is_rejected() {
        let nan = v(f32::NAN, 0.0, 0.0);
        assert!(sweep_sphere_sphere(nan, v(1.0, 0.0, 0.0), 1.0, O, 1.0).is_none());
    }
}
