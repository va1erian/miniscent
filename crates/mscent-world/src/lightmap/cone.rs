//! The cone of a projector: a spot light shines one way and fades out at the
//! edge of its beam, so a pool of light has a soft rim and not a hard circle.

use mscent_math::Vec3;

/// Where a light shines. `share` is 1 inside the inner angle, 0 outside the
/// outer one, and a smooth step between them.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cone {
    /// Unit vector the light shines along.
    dir: Vec3,
    cos_inner: f32,
    cos_outer: f32,
}

impl Cone {
    /// A cone about `dir` (of any length) with half angles `inner <= outer`, in
    /// radians within `0..=PI / 2`. `None` for a direction that is zero or not
    /// finite, or angles that are not in order or range.
    pub fn new(dir: Vec3, inner: f32, outer: f32) -> Option<Cone> {
        let half_turn = core::f32::consts::FRAC_PI_2;
        let ok = (0.0..=outer).contains(&inner) && outer > 0.0 && outer <= half_turn;
        if !ok || !dir.is_finite() {
            return None;
        }
        Some(Cone {
            dir: dir.normalized()?,
            cos_inner: inner.cos(),
            cos_outer: outer.cos(),
        })
    }

    pub fn dir(&self) -> Vec3 {
        self.dir
    }

    /// Whether the edges meet, which makes the cone's edge a step: dividing by so
    /// small a span would not be a smooth one. The GPU is told, not left to test.
    pub fn is_hard(&self) -> bool {
        self.cos_inner - self.cos_outer <= f32::EPSILON
    }

    /// The cosines of the inner and outer angles, which are what the cone is.
    pub fn edge_cosines(&self) -> (f32, f32) {
        (self.cos_inner, self.cos_outer)
    }

    /// The fraction of the light that goes along `from_light`, a unit vector
    /// from the light to the point.
    pub fn share(&self, from_light: Vec3) -> f32 {
        let along = self.dir.dot(from_light);
        let span = self.cos_inner - self.cos_outer;
        if self.is_hard() {
            return f32::from(u8::from(along >= self.cos_outer));
        }
        let t = ((along - self.cos_outer) / span).clamp(0.0, 1.0);
        t * t * (3.0 - 2.0 * t)
    }

    /// Whether the cone lights nothing at `from_light`, which is the cheap
    /// early-out before a shadow ray.
    pub fn misses(&self, from_light: Vec3) -> bool {
        self.dir.dot(from_light) <= self.cos_outer
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn z_cone(inner: f32, outer: f32) -> Cone {
        Cone::new(Vec3::new(0.0, 0.0, 2.0), inner, outer).expect("valid")
    }

    fn at_angle(a: f32) -> Vec3 {
        Vec3::new(a.sin(), 0.0, a.cos())
    }

    #[test]
    fn full_inside_the_inner_angle_and_dark_past_the_outer() {
        let c = z_cone(0.2, 0.5);
        assert_eq!(c.share(at_angle(0.0)), 1.0);
        assert_eq!(c.share(at_angle(0.19)), 1.0);
        assert_eq!(c.share(at_angle(0.51)), 0.0);
        assert!(c.misses(at_angle(0.51)));
        assert_eq!(c.share(Vec3::new(0.0, 0.0, -1.0)), 0.0);
    }

    #[test]
    fn falls_smoothly_between_the_angles() {
        let c = z_cone(0.2, 0.5);
        let shares: Vec<f32> = (0..=30)
            .map(|i| c.share(at_angle(0.2 + 0.01 * i as f32)))
            .collect();
        assert!(shares.windows(2).all(|w| w[1] <= w[0]));
        assert!((0.0..1.0).contains(&c.share(at_angle(0.35))));
    }

    #[test]
    fn equal_angles_make_a_hard_edge() {
        let c = z_cone(0.4, 0.4);
        assert_eq!(c.share(at_angle(0.39)), 1.0);
        assert_eq!(c.share(at_angle(0.41)), 0.0);
    }

    #[test]
    fn only_edges_that_meet_make_a_hard_cone() {
        assert!(z_cone(0.4, 0.4).is_hard());
        assert!(!z_cone(0.2, 0.5).is_hard());
    }

    #[test]
    fn refuses_what_is_not_a_cone() {
        let up = Vec3::new(0.0, 0.0, 1.0);
        assert!(Cone::new(Vec3::ZERO, 0.1, 0.2).is_none());
        assert!(Cone::new(Vec3::new(f32::NAN, 0.0, 1.0), 0.1, 0.2).is_none());
        assert!(Cone::new(up, 0.3, 0.2).is_none());
        assert!(Cone::new(up, -0.1, 0.2).is_none());
        assert!(Cone::new(up, 0.0, 0.0).is_none());
        assert!(Cone::new(up, 0.1, 2.0).is_none());
        assert!(Cone::new(up, f32::NAN, 0.2).is_none());
    }
}
