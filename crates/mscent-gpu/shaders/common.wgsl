// What every surface shares: the frame, and the software renderer's integer
// colour maths (mul_rgb, lerp_rgb, dither_of) with its results kept in 0..=255.
// Numbers that tune the look arrive as overrides from their Rust constants.

struct Frame {
    world_to_clip: mat4x4f,
    fog_color: u32,
    fog_start: f32,
    fog_end: f32,
    fog_on: u32,
    bump_on: u32,
    spec_on: u32,
    // The GPU-only looks, from the effects switches (effects.rs).
    parallax_on: u32,
    shadows_on: u32,
    atlas_size: vec2f,
    // The dynamic lights, a position and radius each, then a colour; a light of
    // radius 0 reaches nothing.
    lights: array<vec4f, 4>,
    light_colors: array<vec4f, 4>,
    // The shadow map each light has, by light, or 0xffffffff for none.
    shadow_slots: vec4u,
}

@group(0) @binding(0) var<uniform> frame: Frame;

fn unpack(color: u32) -> vec3f {
    return vec3f(
        f32((color >> 16u) & 255u),
        f32((color >> 8u) & 255u),
        f32(color & 255u),
    );
}

fn mul_rgb(color: vec3f, light: f32) -> vec3f {
    return min(floor(color * floor(light) / 256.0), vec3f(255.0));
}

// Rounds towards the first colour, as an arithmetic shift of a difference does
// towards minus infinity.
fn lerp_rgb(a: vec3f, b: vec3f, t: f32) -> vec3f {
    return a + floor((b - a) * t / 256.0);
}

fn with_fog(color: vec3f, view_depth: f32) -> vec3f {
    if frame.fog_on == 0u {
        return color;
    }
    let along = (view_depth - frame.fog_start) / (frame.fog_end - frame.fog_start);
    return lerp_rgb(color, unpack(frame.fog_color), floor(clamp(along, 0.0, 1.0) * 256.0));
}

override DITHER_X: u32;
override DITHER_Y: u32;
override DITHER_MIX: u32;
override DITHER_FOLD: u32;
override DITHER_DROP: u32;
override DITHER_BITS: u32;

fn dither_of(at: vec2f) -> f32 {
    let x = u32(at.x) * DITHER_X;
    let y = u32(at.y) * DITHER_Y;
    var h = x ^ y;
    h = (h ^ (h >> DITHER_FOLD)) * DITHER_MIX;
    let bits = 1u << DITHER_BITS;
    return f32((h >> DITHER_DROP) & (bits - 1u)) / f32(bits);
}

fn finished(color: vec3f) -> vec4f {
    return vec4f(color / 255.0, 1.0);
}
