//! Bake and look settings of a level renderer.

use mscent_math::Vec3;

/// Bake and look settings.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BuildConfig {
    /// World units per luxel.
    pub luxel_size: f32,
    pub ambient: Vec3,
    /// Multiplier on every point light's colour (the baked light then
    /// saturates the texture, which is what makes bright pools).
    pub light_gain: f32,
    /// Radius of the lamp disc for soft shadows, in world units (0: hard).
    pub light_radius: f32,
    /// Shadow rays per luxel and light for soft shadows.
    pub shadow_samples: u32,
    /// Colours of the sky gradient; the bottom one is also the fog colour.
    pub sky_top: u32,
    pub sky_bottom: u32,
    /// View depths where fog starts and where it is total.
    pub fog_start: f32,
    pub fog_end: f32,
    /// Vertical field of view in radians.
    pub fov_y: f32,
}

impl Default for BuildConfig {
    fn default() -> Self {
        Self {
            luxel_size: 0.125,
            ambient: Vec3::new(0.006, 0.006, 0.009),
            light_gain: 4.6,
            light_radius: 0.3,
            shadow_samples: 64,
            sky_top: 0xff10_2048,
            sky_bottom: 0xff3a_4a78,
            fog_start: 4.0,
            fog_end: 26.0,
            fov_y: 1.3,
        }
    }
}
