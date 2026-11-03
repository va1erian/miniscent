// A level's faces, as LightingShader and FlatShader draw them: per-pixel light
// from the lightmap, bump, specular and fog. See lighting_shader.rs for why.

override SHARP_RATE: f32;
override LONG_AXIS_LIMIT: f32;
override MIN_BLEND: f32;
override BUMP_FULL: f32;
override BUMP_GONE: f32;
override SPEC_GAIN: f32;
override MIN_SPEC: f32;
override MIN_LENGTH: f32;
// Finish::Fine: materials filtered trilinearly and anisotropically from the
// pixel's own footprint, rather than one level a span as software takes.
override FINE: bool;

const HAS_NORMALS: u32 = 1u;
const SPEC_VISIBLE: u32 = 2u;

struct Face {
    to_luxel_x: vec4f, // a, b, c, and the luxels per texel
    to_luxel_y: vec4f, // d, e, f, and a flat face's light; 0 for a lit one
    frame: vec4f,      // UvToLuxel::frame
    origin: vec4f,     // and the luxel size
    tangent: vec4f,    // and u_min
    bitangent: vec4f,  // and v_min
    normal: vec4f,     // and the mean specular mask
    dir_rect: vec4f,   // where the directions are in the atlas
    first_level: u32,
    level_count: u32,
    flags: u32,
    spec_row: u32,
}

// World to where the mover is now, and the eye in the space it was authored in.
struct Pose {
    to_world: mat4x4f,
    eye: vec4f,
}

@group(0) @binding(1) var<storage, read> faces: array<Face>;
@group(0) @binding(2) var<storage, read> levels: array<vec4f>; // x, y, max x, max y
@group(0) @binding(3) var<storage, read> poses: array<Pose>;
@group(0) @binding(4) var<storage, read> spec_tables: array<f32>;
@group(0) @binding(5) var atlas: texture_2d<f32>;
@group(0) @binding(6) var smooth_clamped: sampler;
@group(0) @binding(7) var smooth_repeat: sampler;
@group(0) @binding(8) var<storage, read> face_lights: array<u32>; // a bit a light
@group(1) @binding(0) var albedo: texture_2d<f32>;
@group(1) @binding(1) var normals: texture_2d<f32>;

struct Corner {
    @builtin(position) clip: vec4f,
    @location(0) texels: vec2f,
    @location(1) view_depth: f32,
    @location(2) @interpolate(flat) face: u32,
    @location(3) @interpolate(flat) pose: u32,
}

@vertex
fn vs(
    @location(0) pos: vec3f,
    @location(1) texels: vec2f,
    @location(2) face: u32,
    @builtin(instance_index) pose: u32,
) -> Corner {
    let world = poses[pose].to_world * vec4f(pos, 1.0);
    let clip = frame.world_to_clip * world;
    return Corner(clip, texels, clip.w, face, pose);
}

// The largest texel step per pixel, with the long axis held to a limit against
// the short: mip_level in raster.rs.
fn texel_rate(texels: vec2f) -> f32 {
    let dx = dpdx(texels);
    let dy = dpdy(texels);
    let a = max(abs(dx.x), abs(dx.y));
    let b = max(abs(dy.x), abs(dy.y));
    return max(min(max(a, b), min(a, b) * LONG_AXIS_LIMIT), max(a, b) / LONG_AXIS_LIMIT);
}

fn albedo_level(rate: f32) -> f32 {
    if rate <= SHARP_RATE {
        return 0.0;
    }
    return min(ceil(log2(rate / SHARP_RATE)), f32(textureNumLevels(albedo) - 1u));
}

// A fetch inside one rectangle of the atlas: faces never bleed into each other.
fn fetch(rect: vec4f, at: vec2f) -> vec3f {
    let inside = clamp(at - vec2f(0.5), vec2f(0.0), rect.zw);
    let where_in_atlas = (rect.xy + inside + vec2f(0.5)) / frame.atlas_size;
    return textureSampleLevel(atlas, smooth_clamped, where_in_atlas, 0.0).rgb;
}

fn fast_log2(x: f32) -> f32 {
    return f32(bitcast<u32>(x)) / 8388608.0 - 127.0;
}

// The baked colour here, blended between two levels where a pixel covers more
// than a luxel: light_sampler.rs.
fn light_color(face: Face, luxel: vec2f, rate: f32) -> vec3f {
    let luxels = rate * face.to_luxel_x.w;
    let top = f32(face.level_count - 1u);
    let lod = clamp(select(0.0, fast_log2(luxels), luxels > 1.0), 0.0, top);
    let fine = floor(lod);
    let fraction = lod - fine;
    let index = face.first_level + u32(fine);
    let near = fetch(levels[index], luxel / pow(2.0, fine));
    if fraction < MIN_BLEND || fine >= top {
        return near;
    }
    let far = fetch(levels[index + 1u], luxel / pow(2.0, fine + 1.0));
    return near + (far - near) * fraction;
}

fn to_texture_frame(face: Face, v: vec3f) -> vec3f {
    let f = face.frame;
    return vec3f(v.x * f.x + v.y * f.y, v.x * f.z + v.y * f.w, v.z);
}

// The highlight: `add_highlight` in texel_light.rs.
fn with_highlight(out: vec3f, face: Face, corner: Corner, k: vec3f, n: vec3f, tilt: vec3f, luxel: vec2f, sampled_mask: f32, relief: f32) -> vec3f {
    let eye = poses[corner.pose].eye.xyz - face.origin.xyz;
    let eye_tangent = vec3f(dot(eye, face.tangent.xyz), dot(eye, face.bitangent.xyz), dot(eye, face.normal.xyz));
    let on_face = vec3f(face.tangent.w + luxel.x * face.origin.w, face.bitangent.w + luxel.y * face.origin.w, 0.0);
    let flat_normal = vec3f(0.0, 0.0, 1.0);
    let view = unit_or(eye_tangent - on_face, flat_normal);
    let light = unit_or(tilt, flat_normal);
    let half_vector = to_texture_frame(face, unit_or(light + view, light));
    let s = spec_response(face.spec_row, dot(n, half_vector) * (1.5 - 0.5 * dot(half_vector, half_vector)));
    if s <= MIN_SPEC {
        return out;
    }
    let mean = face.normal.w;
    var mask = 1.0;
    if mean < 1.0 {
        mask = mean + (sampled_mask - mean) * relief;
    }
    return min(out + floor(k * (s * mask * SPEC_GAIN)), vec3f(255.0));
}

// A material's texel at `at`, the derivatives of `at` across the pixel in `grad`.
fn material(t: texture_2d<f32>, at: vec2f, grad: mat2x2f, level: f32) -> vec4f {
    if FINE {
        return textureSampleGrad(t, smooth_repeat, at, grad[0], grad[1]);
    }
    return textureSampleLevel(t, smooth_repeat, at, level);
}

// FacePoints::at in face_points.rs, in the face's tangent frame, at the pixel
// rather than stepped along a span.
fn dynamic_light(face: Face, mask: u32, luxel: vec2f) -> vec3f {
    let at = vec2f(face.tangent.w + luxel.x * face.origin.w, face.bitangent.w + luxel.y * face.origin.w);
    var sum = vec3f(0.0);
    var left = mask;
    while left != 0u {
        let i = firstTrailingBit(left);
        left &= left - 1u;
        let l = frame.lights[i];
        let rel = l.xyz - face.origin.xyz;
        let to = vec3f(dot(rel, face.tangent.xyz) - at.x, dot(rel, face.bitangent.xyz) - at.y, dot(rel, face.normal.xyz));
        let dist = length(to);
        if dist < l.w && dist > MIN_LENGTH {
            let fall = 1.0 - dist / l.w;
            sum += frame.light_colors[i].rgb * 256.0 * (max(to.z / dist, 0.0) * fall * fall);
        }
    }
    return sum;
}

fn lit(face: Face, corner: Corner, texel: vec3f, rate: f32, grad: mat2x2f) -> vec3f {
    let u_v = vec3f(corner.texels, 1.0);
    let luxel = vec2f(dot(face.to_luxel_x.xyz, u_v), dot(face.to_luxel_y.xyz, u_v));
    var k = light_color(face, luxel, rate) * 256.0;
    let mask = face_lights[corner.face];
    if mask != 0u {
        k += dynamic_light(face, mask, luxel);
    }
    let baked_dir = fetch(face.dir_rect, luxel);
    let relief = clamp((BUMP_GONE - rate) / (BUMP_GONE - BUMP_FULL), 0.0, 1.0);
    var n = vec3f(0.0, 0.0, 1.0);
    var sampled_mask = face.normal.w;
    let bumped = frame.bump_on != 0u && (face.flags & HAS_NORMALS) != 0u && relief > 0.0;
    if bumped {
        let at = corner.texels / vec2f(textureDimensions(albedo));
        let sampled = material(normals, at, grad, 0.0);
        sampled_mask = sampled.a;
        n = mix(vec3f(0.0, 0.0, 1.0), sampled.rgb * 2.0 - vec3f(1.0), relief);
        k *= bump_gain(n, to_texture_frame(face, baked_dir));
    }
    k = soft_limit(k);
    let dither = dither_of(corner.clip.xy);
    var out = min(floor(texel * k / 256.0 + vec3f(dither)), vec3f(255.0));
    if frame.spec_on != 0u && (face.flags & SPEC_VISIBLE) != 0u {
        out = with_highlight(out, face, corner, k, n, baked_dir, luxel, sampled_mask, relief);
    }
    return out;
}

@fragment
fn fs(corner: Corner) -> @location(0) vec4f {
    let face = faces[corner.face];
    let rate = texel_rate(corner.texels);
    let at = corner.texels / vec2f(textureDimensions(albedo));
    // Taken here, where every pixel of the quad runs alike, as derivatives must be.
    let grad = mat2x2f(dpdx(at), dpdy(at));
    let sampled = material(albedo, at, grad, albedo_level(rate));
    let texel = round(sampled.rgb * 255.0);
    var out: vec3f;
    if face.to_luxel_y.w > 0.0 {
        out = mul_rgb(texel, face.to_luxel_y.w);
    } else {
        out = lit(face, corner, texel, rate, grad);
    }
    return finished(with_fog(out, corner.view_depth));
}
