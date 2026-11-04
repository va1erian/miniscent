// A receiver's look-up in a light's cube map. Soft, because the owner has turned
// down shadow maps that crawl and jag: the kernel of a percentage-closer filter is
// widened by how far the receiver is from what blocks it, as a light of some size
// would soften it. Its taps are one fixed pattern, not turned per pixel: a random
// turn trades the pattern's steps for grain, ten times the stair-step of the edge
// when measured (tests/shadows.rs), and a pattern of this many taps leaves steps
// of a thirty-second of the light, which a penumbra many pixels wide hides. The
// bias is three terms: the receiver is moved out along its normal, its depth is taken on its
// own plane at each tap and not at one point, and a small slope-scaled term is
// left over for the plane being sampled at a texel's resolution.

// Maps in all, the first SHADOW_LIGHTS of them the dynamic lights', then those of
// LAMP_SHADOWS static lamps, whose baked light falls off with a power of the
// distance (the baker's).
override SHADOW_MAPS: u32;
override SHADOW_LIGHTS: u32;
override LAMP_SHADOWS: u32;
override LAMP_FALLOFF_POWER: f32;
override SHADOW_SIDE: f32;
override SHADOW_NORMAL_OFFSET: f32;
override SHADOW_BIAS: f32;
override SHADOW_SLOPE_BIAS: f32;
override SHADOW_LIGHT_SIZE: f32;
override SHADOW_MIN_KERNEL: f32;
override SHADOW_MAX_KERNEL: f32;
override SHADOW_BLOCKER_TAPS: u32;
override SHADOW_TAPS: u32;

@group(0) @binding(9) var shadow_maps: texture_cube_array<f32>;
@group(0) @binding(10) var shadow_sampler: sampler;

const GOLDEN_ANGLE: f32 = 2.3999632;

// Tap `i` of `n` of a disc of radius 1.
fn shadow_tap(i: u32, n: u32) -> vec2f {
    let r = sqrt((f32(i) + 0.5) / f32(n));
    let a = f32(i) * GOLDEN_ANGLE;
    return vec2f(cos(a), sin(a)) * r;
}

// The distance over the radius of what is nearest the light along `dir`.
fn nearest(slot: u32, dir: vec3f) -> f32 {
    return textureSampleLevel(shadow_maps, shadow_sampler, dir, slot, 0.0).r;
}

// Where `to` (from the light to the receiver) on a plane facing `n` has its depth
// along the unit direction `dir`, over `reach`.
fn on_plane(to: vec3f, n: vec3f, dir: vec3f, reach: f32) -> f32 {
    // Both dots are negative for a receiver that faces the light.
    let facing = min(dot(n, dir), -0.05);
    return min(dot(n, to) / facing, 4.0 * length(to)) / reach;
}

// How much of the light, 0 to 1, reaches `p` from light `slot` at `light` (a
// position and a radius), with the receiver's plane facing `n`.
fn shadow_visibility(slot: u32, light: vec4f, p: vec3f, n: vec3f) -> f32 {
    let to = p - light.xyz;
    let dist = length(to);
    if dist <= MIN_LENGTH {
        return 1.0;
    }
    // One texel of the map's face, as a length at the receiver, and as an angle.
    let step = 2.0 / SHADOW_SIDE;
    let texel = dist * step;
    let cos_t = clamp(dot(n, -to / dist), 0.0, 1.0);
    let sin_t = sqrt(1.0 - cos_t * cos_t);
    let moved = to + n * (texel * SHADOW_NORMAL_OFFSET * sin_t);
    let d = length(moved);
    let dir = moved / d;
    let tilt = min(sin_t / max(cos_t, 0.05), 8.0);
    let bias = texel * (SHADOW_BIAS + SHADOW_SLOPE_BIAS * tilt) / light.w;
    let up = select(vec3f(0.0, 0.0, 1.0), vec3f(1.0, 0.0, 0.0), abs(dir.z) > 0.9);
    let across = normalize(cross(dir, up));
    let down = cross(dir, across);
    // What is in the way, within the widest kernel.
    var blocked = 0.0;
    var count = 0.0;
    for (var i = 0u; i < SHADOW_BLOCKER_TAPS; i++) {
        let o = shadow_tap(i, SHADOW_BLOCKER_TAPS) * (SHADOW_MAX_KERNEL * step);
        let at = normalize(dir + across * o.x + down * o.y);
        let depth = nearest(slot, at);
        if depth < on_plane(moved, n, at, light.w) - bias {
            blocked += depth;
            count += 1.0;
        }
    }
    if count == 0.0 {
        return 1.0;
    }
    let width = SHADOW_LIGHT_SIZE * (d - blocked / count * light.w) / max(blocked / count * light.w, MIN_LENGTH);
    // The kernel's radius is half the penumbra's width.
    let kernel = clamp(0.5 * width / texel, SHADOW_MIN_KERNEL, SHADOW_MAX_KERNEL);
    var seen = 0.0;
    for (var i = 0u; i < SHADOW_TAPS; i++) {
        let o = shadow_tap(i, SHADOW_TAPS) * (kernel * step);
        let at = normalize(dir + across * o.x + down * o.y);
        if nearest(slot, at) >= on_plane(moved, n, at, light.w) - bias {
            seen += 1.0;
        }
    }
    return seen / f32(SHADOW_TAPS);
}
