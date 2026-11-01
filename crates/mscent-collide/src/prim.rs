//! Analytic ray tests against spheres and cylinders. A "ray" here is the path
//! of a swept sphere's centre, `from + t * d` for `t` in `[0, 1]`, and the
//! target is inflated by the sweep radius, so these give exact contact times.

use mscent_math::Vec3d;

/// A first contact: time along the sweep and the unit surface normal at the
/// contact (pointing from the obstacle toward the swept centre).
#[derive(Clone, Copy, Debug)]
pub(crate) struct Contact {
    pub t: f64,
    pub normal: Vec3d,
}

/// Smallest `t` in `[0, 1]` where `a t^2 + 2 b t + c` first drops to zero,
/// entering from outside (`c > 0`). Already inside (`c <= 0`) counts as an
/// immediate contact only while moving deeper (`b < 0`), so a sphere may
/// leave an embedded position freely.
fn first_entry(a: f64, b: f64, c: f64) -> Option<f64> {
    if c <= 0.0 {
        return (b < 0.0).then_some(0.0);
    }
    if b >= 0.0 || a <= 0.0 {
        return None;
    }
    let disc = b * b - a * c;
    if disc < 0.0 {
        return None;
    }
    let t = (-b - disc.sqrt()) / a;
    (t <= 1.0).then_some(t.max(0.0))
}

/// Centre path `from + t d` against a sphere of `radius` around `centre`
/// (the obstacle already inflated by the mover's radius).
pub(crate) fn ray_sphere(from: Vec3d, d: Vec3d, centre: Vec3d, radius: f64) -> Option<Contact> {
    let w = from - centre;
    let t = first_entry(d.dot(d), w.dot(d), w.dot(w) - radius * radius)?;
    let normal = (from + d * t - centre).normalized()?;
    Some(Contact { t, normal })
}

/// Centre path against the cylinder of `radius` around segment `a`-`b`. Only
/// the cylindrical part counts: hits beyond the end caps belong to the
/// vertex spheres.
pub(crate) fn ray_segment_cylinder(
    from: Vec3d,
    d: Vec3d,
    a: Vec3d,
    b: Vec3d,
    radius: f64,
) -> Option<Contact> {
    let axis = b - a;
    let len = axis.length();
    let u = axis.normalized()?;
    let w = from - a;
    let wp = w - u * w.dot(u);
    let dp = d - u * d.dot(u);
    let t = first_entry(dp.dot(dp), wp.dot(dp), wp.dot(wp) - radius * radius)?;
    let p = from + d * t;
    let s = (p - a).dot(u);
    if !(0.0..=len).contains(&s) {
        return None;
    }
    let normal = (p - (a + u * s)).normalized()?;
    Some(Contact { t, normal })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(x: f64, y: f64, z: f64) -> Vec3d {
        Vec3d::new(x, y, z)
    }

    #[test]
    fn sphere_head_on_and_miss() {
        let c = ray_sphere(v(-5.0, 0.0, 0.0), v(10.0, 0.0, 0.0), v(0.0, 0.0, 0.0), 1.0).unwrap();
        assert!((c.t - 0.4).abs() < 1e-12);
        assert!((c.normal.x + 1.0).abs() < 1e-12);
        assert!(ray_sphere(v(-5.0, 1.5, 0.0), v(10.0, 0.0, 0.0), v(0.0, 0.0, 0.0), 1.0).is_none());
    }

    #[test]
    fn sphere_embedded_only_when_moving_in() {
        let o = v(0.0, 0.0, 0.0);
        let inward = ray_sphere(v(0.5, 0.0, 0.0), v(-1.0, 0.0, 0.0), o, 1.0).unwrap();
        assert_eq!(inward.t, 0.0);
        assert!(ray_sphere(v(0.5, 0.0, 0.0), v(1.0, 0.0, 0.0), o, 1.0).is_none());
    }

    #[test]
    fn cylinder_side_hit_and_cap_exclusion() {
        let (a, b) = (v(0.0, 0.0, -2.0), v(0.0, 0.0, 2.0));
        let c = ray_segment_cylinder(v(-5.0, 0.0, 0.0), v(10.0, 0.0, 0.0), a, b, 1.0).unwrap();
        assert!((c.t - 0.4).abs() < 1e-12);
        // Passing beyond the end of the segment is the vertex sphere's job.
        assert!(ray_segment_cylinder(v(-5.0, 0.0, 2.5), v(10.0, 0.0, 0.0), a, b, 1.0).is_none());
        // Parallel to the axis never enters the cylinder wall.
        assert!(ray_segment_cylinder(v(3.0, 0.0, -9.0), v(0.0, 0.0, 18.0), a, b, 1.0).is_none());
    }
}
