//! What blocks light: the BSP, and the surfaces of voxel volumes. Voxels shadow
//! brushes and brushes shadow voxels, because every shadow ray, whoever fires
//! it, asks this one type. It also holds the sky ([`Sky`]): the lamps of vista
//! rooms, whose shadow rays cross a window into their room.

use mscent_math::Vec3d;

use super::ray::segment_clear;
use super::sky::Sky;
use super::voxel_rays::VolumeRays;
use crate::bsp::Bsp;
use crate::voxel::VoxelWorld;

/// The BSP and, optionally, the volumes of a level.
pub struct Occluders<'a> {
    bsp: &'a Bsp,
    volumes: Vec<VolumeRays>,
    world: Option<&'a VoxelWorld>,
    sky: Sky,
}

impl<'a> Occluders<'a> {
    /// Brushes only. With no volumes this is exactly the BSP segment query, so a
    /// level without volumes bakes the same bytes it always did.
    pub fn brushes(bsp: &'a Bsp) -> Self {
        Self {
            bsp,
            volumes: Vec::new(),
            world: None,
            sky: Sky::default(),
        }
    }

    /// Brushes and the full-detail surfaces of `world`.
    pub fn new(bsp: &'a Bsp, world: &'a VoxelWorld) -> Self {
        let volumes = world
            .volumes
            .iter()
            .filter_map(|geometry| VolumeRays::new(geometry, &world.chunks))
            .collect();
        Self {
            bsp,
            volumes,
            world: Some(world),
            sky: Sky::default(),
        }
    }

    /// The same occluders, with `sky` lighting the play space through its windows.
    pub fn with_sky(self, sky: Sky) -> Self {
        Self { sky, ..self }
    }

    pub fn sky(&self) -> &Sky {
        &self.sky
    }

    pub fn has_volumes(&self) -> bool {
        !self.volumes.is_empty()
    }

    /// True if the segment `a`..`b` passes only through air: empty BSP leaves,
    /// then no voxel triangle. Non-finite ends are blocked.
    pub fn segment_clear(&self, a: Vec3d, b: Vec3d) -> bool {
        segment_clear(self.bsp, a, b) && self.volumes.iter().all(|v| v.clear(a, b))
    }

    /// Whether `p` is inside rock of either kind: no light can be read there. On
    /// a thin or sharply curved shape, a cell's lifted point can land inside.
    pub fn in_solid(&self, p: Vec3d) -> bool {
        !p.is_finite() || self.bsp.is_solid(p) || self.world.is_some_and(|w| w.is_solid(p))
    }
}
