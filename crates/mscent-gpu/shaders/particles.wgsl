// Particles over the finished scene, as particles/ in the software renderer
// draws them: decals, then "over" sprites back to front, then additive sprites
// and streaks. Each reads the scene's 1/z under it and tests it as software
// does; none writes depth. Where each lands and which texel it reads comes from
// `particles::plan`, so only the per-pixel maths is here. A pipeline puts
// `scene` and `scene_depth` ahead of this: the depth, or a multisampled one
// resolved to one value a pixel.

override DECAL_DEPTH_BIAS: f32;

struct Particles {
    size: vec2f,
    // a and k of depth = a / z - k, from Camera::gpu_depth_terms.
    depth_terms: vec2f,
}

struct Sprite {
    pixels: vec4f, // first column and row, and past the last of each
    origin: vec2f,
    scale: f32,
    inv_z: f32,
    tint: u32,
    start: u32, // where its mip starts in the bank
    side: u32,  // 0 for a streak's pixel, which covers `cover`
    cover: u32,
}

struct Decal {
    origin: vec4f, // and two unused
    base: vec4f,   // 1/z, s/z and t/z at the origin
    gx: vec4f,
    gy: vec4f,
    start: u32,
    side: u32,
    darkness: u32,
    pad: u32,
}

@group(0) @binding(0) var<uniform> particles: Particles;
@group(0) @binding(1) var<storage, read> bank: array<u32>; // four mask bytes a word
@group(0) @binding(2) var<storage, read> sprites: array<Sprite>;
@group(0) @binding(3) var<storage, read> decals: array<Decal>;
@group(0) @binding(4) var<storage, read> corners: array<vec4f>; // x, y, decal

// The scene's 1/z at `pixel` as the software z-buffer holds it, 0 for none.
fn scene_inv_z(pixel: vec2u) -> f32 {
    let d = scene_depth(pixel);
    if d <= 0.0 {
        return 0.0;
    }
    return (d + particles.depth_terms.y) / particles.depth_terms.x;
}

fn mask_byte(at: u32) -> u32 {
    return (bank[at >> 2u] >> ((at & 3u) * 8u)) & 255u;
}

fn to_clip(pixel: vec2f) -> vec4f {
    let ndc = pixel / particles.size * 2.0 - vec2f(1.0);
    return vec4f(ndc.x, -ndc.y, 0.0, 1.0);
}

struct SpriteCorner {
    @builtin(position) clip: vec4f,
    @location(0) @interpolate(flat) index: u32,
}

@vertex
fn vs_sprite(@builtin(vertex_index) corner: u32, @builtin(instance_index) index: u32) -> SpriteCorner {
    let p = sprites[index].pixels;
    let xs = array(p.x, p.z, p.x, p.z, p.z, p.x);
    let ys = array(p.y, p.y, p.w, p.y, p.w, p.w);
    return SpriteCorner(to_clip(vec2f(xs[corner], ys[corner])), index);
}

// `nearest` in sprite.rs.
fn nearest(pos: f32, scale: f32, side: u32) -> u32 {
    return min(u32(max(pos * scale, 0.0)), side - 1u);
}

// The coverage of `s` at `pixel`, past software's depth test, and its tint
// scaled by it: `s = m * tint / 256` of kernels.rs.
fn sprite_texel(s: Sprite, pixel: vec2u) -> vec4f {
    if !(s.inv_z >= scene_inv_z(pixel)) {
        discard;
    }
    var m = s.cover;
    if s.side > 0u {
        let centre = vec2f(pixel) + vec2f(0.5) - s.origin;
        let u = nearest(centre.x, s.scale, s.side);
        let v = nearest(centre.y, s.scale, s.side);
        m = mask_byte(s.start + v * s.side + u);
    }
    let tint = unpack(s.tint);
    return vec4f(floor(f32(m) * tint / 256.0), floor(f32(m) * f32(s.tint >> 24u) / 256.0));
}

fn unpack(color: u32) -> vec3f {
    return vec3f(f32((color >> 16u) & 255u), f32((color >> 8u) & 255u), f32(color & 255u));
}

// add_px: the hardware adds the colour and keeps the alpha.
@fragment
fn fs_add(corner: SpriteCorner) -> @location(0) vec4f {
    let t = sprite_texel(sprites[corner.index], vec2u(corner.clip.xy));
    return vec4f(t.rgb / 255.0, 0.0);
}

// over_px: the colour, and an alpha that keeps `256 - m * alpha / 256` of what
// is under it.
@fragment
fn fs_over(corner: SpriteCorner) -> @location(0) vec4f {
    let t = sprite_texel(sprites[corner.index], vec2u(corner.clip.xy));
    return vec4f(t.rgb / 255.0, t.a / 256.0);
}

struct DecalCorner {
    @builtin(position) clip: vec4f,
    @location(0) @interpolate(flat) index: u32,
}

@vertex
fn vs_decal(@builtin(vertex_index) at: u32) -> DecalCorner {
    let c = corners[at];
    return DecalCorner(to_clip(c.xy), bitcast<u32>(c.z));
}

// shade_row in decal.rs, with the mask position exact at every pixel: the
// hardware keeps `keep / 256` of the colour under it.
@fragment
fn fs_decal(corner: DecalCorner) -> @location(0) vec4f {
    let d = decals[corner.index];
    let pixel = vec2u(corner.clip.xy);
    let offset = vec2f(pixel) + vec2f(0.5) - d.origin.xy;
    let at = d.base.xyz + d.gx.xyz * offset.x + d.gy.xyz * offset.y;
    let z = scene_inv_z(pixel);
    if !(z > 0.0 && at.x * (1.0 + DECAL_DEPTH_BIAS) >= z) {
        discard;
    }
    let scale = f32(d.side) / at.x;
    let last = i32(d.side) - 1;
    let u = u32(clamp(i32(at.y * scale), 0, last));
    let v = u32(clamp(i32(at.z * scale), 0, last));
    let m = mask_byte(d.start + v * d.side + u);
    let keep = 256u - ((m * d.darkness) >> 8u);
    if keep >= 256u {
        discard;
    }
    return vec4f(vec3f(f32(keep) / 256.0), 1.0);
}
