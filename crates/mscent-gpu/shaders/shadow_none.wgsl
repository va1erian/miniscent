// The shadow lookup for a device that cannot hold the maps' cube array
// (shadows.rs, `cube_arrays`): no light has a map, so none is looked up, and the
// faces draw as they do with the shadows switched off.

const SHADOW_MAPS: u32 = 0u;
const SHADOW_LIGHTS: u32 = 0u;
const LAMP_SHADOWS: u32 = 0u;
const LAMP_FALLOFF_POWER: f32 = 1.0;
// What the faces' shader reads of the lookup's tuning, never reached with no lamps.
const SHADOW_SIDE: f32 = 1.0;
const SHADOW_NORMAL_OFFSET: f32 = 0.0;
const SHADOW_MAX_KERNEL: f32 = 0.0;

fn shadow_visibility(slot: u32, light: vec4f, p: vec3f, n: vec3f) -> f32 {
    return 1.0;
}
