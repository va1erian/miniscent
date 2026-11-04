// The shadow lookup for a device that cannot hold the maps' cube array
// (shadows.rs, `cube_arrays`): no light has a map, so none is looked up, and the
// faces draw as they do with the shadows switched off.

const SHADOW_LIGHTS: u32 = 0u;

fn shadow_visibility(slot: u32, light: vec4f, p: vec3f, n: vec3f) -> f32 {
    return 1.0;
}
