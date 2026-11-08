//! A projector's cone in an orthographic view: the base of the beam at the
//! light's radius and the rays out to it. Worked out in world space and then
//! flattened to the view's two axes, so it is right from the top and from the
//! side, and a projector aimed along the view shows as the circle it is.

use mscent_math::Vec3;
use mscent_world::Entity;
use mscent_world::entity::projector_dir;
use xui::Color;
use xui::geometry::Point;

use super::{Pen, fill_convex, rgba};

/// Segments in the outline of the base; a multiple of 4 so four rays land on it.
const SEGMENTS: usize = 24;

/// A line in the view's two axes.
pub(super) type Segment = ([f64; 2], [f64; 2]);

/// The lines of the cone of half angle `outer` from `apex` along unit `dir`,
/// `length` long, flattened to world axes `axes`.
pub(super) fn segments(
    axes: (usize, usize),
    (apex, dir): (Vec3, Vec3),
    (length, outer): (f32, f32),
) -> Vec<Segment> {
    let flat = |p: Vec3| {
        let c = [p.x, p.y, p.z];
        [f64::from(c[axes.0]), f64::from(c[axes.1])]
    };
    // Any vector across the axis will do to start the circle.
    let helper = if dir.z.abs() < 0.9 {
        Vec3::new(0.0, 0.0, 1.0)
    } else {
        Vec3::new(1.0, 0.0, 0.0)
    };
    let Some(u) = dir.cross(helper).normalized() else {
        return Vec::new();
    };
    let v = dir.cross(u);
    let (side, along) = outer.sin_cos();
    let rim = |k: usize| {
        let t = (k % SEGMENTS) as f32 * core::f32::consts::TAU / SEGMENTS as f32;
        flat(apex + (dir * along + (u * t.cos() + v * t.sin()) * side) * length)
    };
    let mut out: Vec<Segment> = (0..SEGMENTS).map(|k| (rim(k), rim(k + 1))).collect();
    let from = flat(apex);
    out.extend((0..SEGMENTS).step_by(SEGMENTS / 4).map(|k| (from, rim(k))));
    out.push((from, flat(apex + dir * length)));
    out
}

/// Draws a projector at `c`: a triangle, to tell it from a point light's disc
/// and a goal's diamond, and
/// for the selected one (`Some(view axes)`) its cone as well.
pub(super) fn projector(
    pen: &mut Pen,
    (c, r): (Point, f32),
    e: &Entity,
    cone: Option<(usize, usize)>,
) {
    let Entity::Projector {
        yaw,
        pitch,
        color,
        radius,
        outer,
        ..
    } = *e
    else {
        return;
    };
    let col = Color::rgb(
        (color.x * 255.0) as u8,
        (color.y * 255.0) as u8,
        (color.z * 255.0) as u8,
    );
    if let Some(axes) = cone.filter(|_| (radius as f64 * pen.cam.scale) < 4000.0) {
        for (p, q) in segments(axes, (e.pos(), projector_dir(yaw, pitch)), (radius, outer)) {
            pen.canvas.draw_line(pen.pt(p), pen.pt(q), col, 1.0);
        }
    }
    let d = (r * 1.2) as i32;
    let corners = [(0, -d), (d, d), (-d, d)];
    let pts: Vec<Point> = corners
        .iter()
        .map(|&(x, y)| Point::new(c.x + x, c.y + y))
        .collect();
    fill_convex(pen, &pts, rgba(col, 255));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_projector_aimed_along_the_view_is_a_circle_of_the_beam_s_width() {
        let down = Vec3::new(0.0, 0.0, -1.0);
        let lines = segments((0, 1), (Vec3::ZERO, down), (10.0, 0.5));
        let want = 10.0 * f64::from(0.5_f32.sin());
        // Every rim point is `want` from the axis, which in this view is the apex.
        for (from, _) in &lines[..SEGMENTS] {
            let d = from[0].hypot(from[1]);
            assert!((d - want).abs() < 1e-4, "{d} against {want}");
        }
    }

    #[test]
    fn seen_from_the_side_the_axis_runs_the_length() {
        let down = Vec3::new(0.0, 0.0, -1.0);
        let lines = segments((0, 2), (Vec3::ZERO, down), (10.0, 0.5));
        let (from, to) = lines[lines.len() - 1];
        assert_eq!(from, [0.0, 0.0]);
        assert!((to[1] + 10.0).abs() < 1e-4);
    }

    #[test]
    fn a_zero_direction_draws_nothing() {
        assert!(segments((0, 1), (Vec3::ZERO, Vec3::ZERO), (1.0, 0.5)).is_empty());
    }
}
