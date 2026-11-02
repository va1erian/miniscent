//! What the particles collide with: the BSP, and the voxel volumes if the
//! level has any.
//!
//! A BSP point test costs about 0.1 us and a voxel one about 8 us, so the two
//! are not interchangeable in a per-tick budget. [`FxWorld::in_volume`] says
//! whether a test at a point is the dear kind, and a point outside every volume box
//! costs what it always did: a level with no volumes behaves as it did before.

use mscent_math::epsilon::PARTICLE_VOXEL_NORMAL_REACH;
use mscent_math::{Vec3, Vec3d};
use mscent_world::Bsp;
use mscent_world::voxel::VoxelWorld;

/// Solid and empty space as the particle system sees it. Cheap to copy.
#[derive(Clone, Copy, Debug)]
pub struct FxWorld<'a> {
    bsp: &'a Bsp,
    voxels: Option<&'a VoxelWorld>,
}

impl<'a> From<&'a Bsp> for FxWorld<'a> {
    fn from(bsp: &'a Bsp) -> Self {
        Self { bsp, voxels: None }
    }
}

impl<'a> FxWorld<'a> {
    /// The BSP plus `voxels`. A world with no volumes is dropped here, so a
    /// level without any is exactly the plain BSP.
    pub fn new(bsp: &'a Bsp, voxels: &'a VoxelWorld) -> Self {
        Self {
            bsp,
            voxels: (!voxels.volumes.is_empty()).then_some(voxels),
        }
    }

    /// Whether the BSP or a volume makes `p` solid. The BSP is asked first
    /// and the volumes only if it says empty.
    pub fn is_solid(&self, p: Vec3d) -> bool {
        self.bsp.is_solid(p) || self.voxels.is_some_and(|v| v.is_solid(p))
    }

    /// The normal of the voxel surface at `at`, out of the rock, if one lies
    /// within [`PARTICLE_VOXEL_NORMAL_REACH`]. `None` for a brush wall.
    pub fn voxel_normal(&self, at: Vec3) -> Option<Vec3> {
        let reach = f64::from(PARTICLE_VOXEL_NORMAL_REACH);
        let n = self.voxels?.surface_normal(at.to_f64(), reach)?;
        Some(n.to_f32()).filter(|n| n.is_finite())
    }

    /// Whether `p` is in some volume's box, where a test is expensive.
    pub fn in_volume(&self, p: Vec3) -> bool {
        self.voxels.is_some_and(|v| v.in_volume(p.to_f64()))
    }

    /// Whether a test at either end of `prev..pos` is expensive.
    pub fn touches_volume(&self, prev: Vec3, pos: Vec3) -> bool {
        self.in_volume(prev) || self.in_volume(pos)
    }
}
