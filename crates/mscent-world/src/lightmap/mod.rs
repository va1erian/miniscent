//! Lightmap pipeline (M5.1 to M5.3, M5.8): planar UVs, atlas packing, a
//! multi-threaded baker with BSP shadow rays, a dominant light direction per
//! luxel and an ambient plus bounce pass.
//!
//! Input is deliberately a small local type ([`LightFace`]: polygon plus
//! normal) so the baker does not depend on how faces are extracted; callers
//! convert their face type with a one-line map.
//!
//! Units: `luxel_size` is world units per luxel; light radii and the normal
//! bias share the world unit. Colours are linear RGB in `Vec3`.

pub mod atlas;
mod bake;
mod bounce;
mod cell_sites;
mod cells;
mod cells_bounce;
mod filter;
mod layout;
mod occluders;
mod parallel;
pub mod ray;
mod soft;
pub mod store;
pub mod uv;
mod voxel_rays;

#[cfg(test)]
mod density_tests;
#[cfg(test)]
mod review_tests;
#[cfg(test)]
mod soft_tests;
#[cfg(test)]
mod store_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod voxel_light_tests;

pub use atlas::{Atlas, AtlasError, PADDING, Rect};
pub use bake::{FaceGrid, Lightmap, MAX_SHADOW_SAMPLES, bake, bake_cancellable, bake_in};
pub use cell_sites::{CellKey, CellSites, shape_key};
pub use cells::{CellLighting, CellRecord, RECORD_BYTES, light_cells};
pub use occluders::Occluders;
pub use uv::{FaceProjection, MAX_FACE_LUXELS};

use mscent_math::Vec3;

/// A planar polygon to be lit.
#[derive(Clone, Debug, PartialEq)]
pub struct LightFace {
    /// Polygon corners, at least three, all on one plane.
    pub vertices: Vec<Vec3>,
    /// Surface normal, the side that receives light. Need not be unit length.
    pub normal: Vec3,
    /// How finely the face is baked.
    pub density: LuxelDensity,
}

/// How finely a face is baked. Every face keeps its place in the bake's input
/// and output whatever its density, so indexes stay aligned.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum LuxelDensity {
    /// At the bake's luxel size.
    #[default]
    Full,
    /// At the bake's luxel size times this factor, for a face seen from afar.
    /// Below 1 or not a number counts as 1, and past [`Self::MAX_COARSE`] as that.
    Coarse(f32),
    /// No luxels: no rays, no bounce, no atlas area. The face has no light map.
    None,
}

impl LuxelDensity {
    /// The coarsest factor a face may ask for, so a luxel size stays finite.
    pub const MAX_COARSE: f32 = 1024.0;

    /// The factor on the luxel size, or `None` for a face with no luxels.
    pub(super) fn scale(self) -> Option<f32> {
        match self {
            Self::Full => Some(1.0),
            Self::Coarse(k) if k.is_nan() => Some(1.0),
            Self::Coarse(k) => Some(k.clamp(1.0, Self::MAX_COARSE)),
            Self::None => None,
        }
    }
}

impl From<&crate::faces::Face> for LightFace {
    fn from(f: &crate::faces::Face) -> Self {
        Self {
            vertices: f.polygon.vertices().iter().map(|v| v.to_f32()).collect(),
            normal: f.plane.normal.to_f32(),
            density: LuxelDensity::Full,
        }
    }
}

/// A point light with a smooth falloff to zero at `radius`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PointLight {
    pub position: Vec3,
    /// Linear RGB, already multiplied by intensity.
    pub color: Vec3,
    /// Distance at which the light reaches zero. Attenuation is
    /// `(1 - d / radius)^3`, times the cosine of the incidence angle.
    pub radius: f32,
}

/// One baked texel: light colour and the dominant direction towards the light.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Luxel {
    pub color: Vec3,
    /// Unit vector in the face tangent frame (`x` along tangent, `y` along
    /// bitangent, `z` along the normal) pointing towards the light.
    /// Light-free luxels hold [`Luxel::FALLBACK_DIR`].
    pub dir: Vec3,
}

impl Luxel {
    /// Direction used where no light arrives: straight along the normal, so
    /// bump mapping degrades to flat shading instead of flickering.
    pub const FALLBACK_DIR: Vec3 = Vec3::new(0.0, 0.0, 1.0);

    /// Black with the fallback direction.
    pub const DARK: Luxel = Luxel {
        color: Vec3::ZERO,
        dir: Self::FALLBACK_DIR,
    };
}

/// Baker settings.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BakeConfig {
    /// World units per luxel. Grown automatically for huge faces, and for the
    /// whole level when it would not fit the atlas otherwise.
    pub luxel_size: f32,
    /// Worker threads, at least one.
    pub workers: usize,
    /// How far shadow rays start off the surface, to avoid self-shadowing.
    pub normal_bias: f32,
    /// Radius of the disc a point light is treated as for shadows, in world
    /// units; 0 makes hard shadows from a single ray.
    pub light_radius: f32,
    /// Shadow rays per luxel and light when `light_radius` is above zero.
    pub shadow_samples: u32,
    /// Flat light added everywhere.
    pub ambient: Vec3,
    /// Fraction of light a surface re-emits in the bounce pass; 0 disables it.
    pub albedo: f32,
    /// Multiplier on the gathered bounce light.
    pub bounce_gain: f32,
    pub atlas_width: u32,
    /// The limit on memory and on the cache file: past it the level coarsens.
    pub atlas_max_height: u32,
}

impl Default for BakeConfig {
    fn default() -> Self {
        Self {
            luxel_size: 8.0,
            workers: std::thread::available_parallelism().map_or(1, usize::from),
            normal_bias: 0.25,
            light_radius: 0.0,
            shadow_samples: 1,
            ambient: Vec3::new(0.03, 0.03, 0.03),
            albedo: 0.5,
            bounce_gain: 1.0,
            atlas_width: 1024,
            // 201 MiB of luxels, under store::MAX_BYTES so such a bake reloads.
            atlas_max_height: 8192,
        }
    }
}

/// Why a bake or a layout step was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LightmapError {
    /// Face has fewer than three vertices, a zero area or a bad normal.
    DegenerateFace(usize),
    /// Non-finite number in a face.
    NonFiniteFace(usize),
    /// Light with a non-finite value or a non-positive radius.
    InvalidLight(usize),
    /// Config value out of range.
    InvalidConfig,
    /// The atlas could not hold every face, even at one luxel each.
    Atlas(AtlasError),
    /// The caller abandoned the bake.
    Cancelled,
}

impl From<AtlasError> for LightmapError {
    fn from(e: AtlasError) -> Self {
        Self::Atlas(e)
    }
}
