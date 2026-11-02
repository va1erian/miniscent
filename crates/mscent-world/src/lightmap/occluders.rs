//! What blocks light: the BSP, and the surfaces of voxel volumes. Voxels shadow
//! brushes and brushes shadow voxels, because every shadow ray, whoever fires
//! it, asks this one type.

use mscent_math::Vec3d;

use super::ray::segment_clear;
use super::voxel_rays::VolumeRays;
use crate::bsp::Bsp;
use crate::voxel::VoxelWorld;

/// The BSP and, optionally, the volumes of a level.
pub struct Occluders<'a> {
    bsp: &'a Bsp,
    volumes: Vec<VolumeRays>,
}

impl<'a> Occluders<'a> {
    /// Brushes only. With no volumes this is exactly the BSP segment query, so a
    /// level without volumes bakes the same bytes it always did.
    pub fn brushes(bsp: &'a Bsp) -> Self {
        Self {
            bsp,
            volumes: Vec::new(),
        }
    }

    /// Brushes and the full-detail surfaces of `world`.
    pub fn new(bsp: &'a Bsp, world: &VoxelWorld) -> Self {
        let volumes = world
            .volumes
            .iter()
            .filter_map(|geometry| VolumeRays::new(geometry, &world.chunks))
            .collect();
        Self { bsp, volumes }
    }

    pub fn has_volumes(&self) -> bool {
        !self.volumes.is_empty()
    }

    /// True if the segment `a`..`b` passes only through air: empty BSP leaves,
    /// then no voxel triangle. Non-finite ends are blocked.
    pub fn segment_clear(&self, a: Vec3d, b: Vec3d) -> bool {
        segment_clear(self.bsp, a, b) && self.volumes.iter().all(|v| v.clear(a, b))
    }

    /// Whether `p` is inside solid brush. A voxel surface does not count: a
    /// point behind one only finds that out when a ray from it is blocked.
    pub fn in_brush_solid(&self, p: Vec3d) -> bool {
        !p.is_finite() || self.bsp.is_solid(p)
    }
}
