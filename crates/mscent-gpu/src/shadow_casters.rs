//! What casts a shadow in a frame, as the lists the shadow passes draw from, and
//! whether a light's sphere meets any of it.

use std::ops::Range;

use mscent_math::Vec3;

/// What a model part contributes: where its triangles are in the corner list, and
/// a sphere in the world that holds it.
pub(crate) struct PartSphere {
    pub(crate) corners: Range<u32>,
    pub(crate) centre: Vec3,
    pub(crate) radius: f32,
}

/// Enemy models' parts: storage the voxel pipeline reads, and a matrix a part.
pub(crate) struct ModelCasters<'a> {
    pub(crate) vertices: &'a wgpu::Buffer,
    pub(crate) corners: &'a wgpu::Buffer,
    pub(crate) worlds: &'a wgpu::Buffer,
    pub(crate) parts: &'a [PartSphere],
}

/// One mover's triangles, in the level's index list for movers, and a sphere in
/// the world that holds them where the mover now is.
pub(crate) struct MoverDraw {
    pub(crate) pose: u32,
    pub(crate) indices: Range<u32>,
    pub(crate) centre: Vec3,
    pub(crate) radius: f32,
}

/// Movers: the faces' vertices, as authored, and their poses.
pub(crate) struct MoverCasters<'a> {
    pub(crate) vertices: &'a wgpu::Buffer,
    pub(crate) indices: &'a wgpu::Buffer,
    pub(crate) poses: &'a wgpu::Buffer,
    pub(crate) draws: &'a [MoverDraw],
}

/// Everything a frame has that casts a shadow.
#[derive(Default)]
pub(crate) struct Casters<'a> {
    /// Actors' vertices, and how many.
    pub(crate) actors: Option<(&'a wgpu::Buffer, u32)>,
    /// A sphere that holds all the actors' triangles.
    pub(crate) actor_bound: Option<(Vec3, f32)>,
    pub(crate) models: Option<ModelCasters<'a>>,
    pub(crate) movers: Option<MoverCasters<'a>>,
}

/// Whether two spheres touch.
pub(crate) fn meet(a: (Vec3, f32), b: (Vec3, f32)) -> bool {
    (a.0 - b.0).length() <= a.1 + b.1
}

/// The smallest sphere known to hold both: one of them, or the one on the line
/// between their centres that touches both.
fn merged(a: (Vec3, f32), b: (Vec3, f32)) -> (Vec3, f32) {
    let apart = (b.0 - a.0).length();
    if apart + b.1 <= a.1 {
        return a;
    }
    if apart + a.1 <= b.1 {
        return b;
    }
    let radius = (apart + a.1 + b.1) / 2.0;
    (a.0 + (b.0 - a.0) * ((radius - a.1) / apart), radius)
}

impl Casters<'_> {
    /// A sphere for each part of what is here: the movers' and model parts' own,
    /// and one round all the actors.
    fn spheres(&self) -> impl Iterator<Item = (Vec3, f32)> + '_ {
        let parts = self.models.iter().flat_map(|m| {
            (m.parts.iter())
                .filter(|p| !p.corners.is_empty())
                .map(|p| (p.centre, p.radius))
        });
        let movers = self.movers.iter().flat_map(|m| {
            (m.draws.iter())
                .filter(|d| !d.indices.is_empty())
                .map(|d| (d.centre, d.radius))
        });
        let actors = self.actors.filter(|&(_, n)| n > 0).and(self.actor_bound);
        parts.chain(movers).chain(actors)
    }

    /// Whether anything here may fall in the sphere of `radius` at `pos`.
    pub(crate) fn reach(&self, pos: Vec3, radius: f32) -> bool {
        self.spheres().any(|s| meet((pos, radius), s))
    }

    /// A sphere that holds all of what falls in the sphere of `radius` at `pos`,
    /// or `None` where nothing does.
    pub(crate) fn bound(&self, pos: Vec3, radius: f32) -> Option<(Vec3, f32)> {
        let near = self.spheres().filter(|&s| meet((pos, radius), s));
        near.reduce(merged)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn holds(outer: (Vec3, f32), inner: (Vec3, f32)) -> bool {
        (outer.0 - inner.0).length() + inner.1 <= outer.1 + 1e-4
    }

    #[test]
    fn a_merged_sphere_holds_both() {
        let spheres = [
            (Vec3::new(0.0, 0.0, 0.0), 1.0),
            (Vec3::new(5.0, 0.0, 0.0), 2.0),
            (Vec3::new(0.0, 0.0, 0.0), 3.0),
            (Vec3::new(1.0, 1.0, 0.0), 0.5),
            (Vec3::new(1.0, 1.0, 0.0), 0.0),
        ];
        for a in spheres {
            for b in spheres {
                let m = merged(a, b);
                assert!(holds(m, a) && holds(m, b), "{a:?} {b:?} -> {m:?}");
            }
        }
        // Two touching spheres of radius 1 on a line are held by one of radius 2.
        let both = merged(
            (Vec3::new(-1.0, 0.0, 0.0), 1.0),
            (Vec3::new(1.0, 0.0, 0.0), 1.0),
        );
        assert!(
            (both.1 - 2.0).abs() < 1e-5 && both.0.length() < 1e-5,
            "{both:?}"
        );
    }
}
