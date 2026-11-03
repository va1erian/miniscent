// Voxel surfaces, as voxel/fill.rs and voxel/blend.rs draw them. Vertices are
// pulled from storage, so that every corner sees its whole triangle: whether it
// is plain, its three looks, and the texel rate the software path takes once per
// triangle. Texels are filtered as texture.rs does, from a buffer of them.

override SEAM_BAND: f32;
override IN_SEAM: u32;
override TEXELS_PER_UNIT: f32;
override MATERIAL_BAND: f32;
override SHARP_RATE: f32;
override LONG_AXIS_LIMIT: f32;
override BUMP_FULL: f32;
override BUMP_GONE: f32;
override SPEC_GAIN: f32;
override MIN_SPEC: f32;
override SPOT_EDGE: u32;
override SPOT_SPACING: f32;
override SPOT_BAND: f32;
override SPOT_MIN_EDGE: f32;
override SPOT_LOST_AT: f32;
override SPOT_MEAN: f32;
override SPOT_COLOR: u32;

const SMOOTH: u32 = 1u;
const SPEC_VISIBLE: u32 = 2u;
const SPOTS: u32 = 4u;
const NO_NORMALS: u32 = 0xffffffffu;
const FLAT_NORMAL: vec3f = vec3f(0.0, 0.0, 1.0);

struct Voxels {
    eye: vec3f,
    focal: f32,
    size: vec2f,
    near: f32,
    axes: array<vec4f, 6>, // u, then v, of each world axis
    bayer: array<vec4u, 4>,
}

struct Material {
    width: u32,
    height: u32,
    mips: u32,
    normals: u32,
    spec_row: u32,
    flags: u32,
    pad: vec2u,
    levels: array<u32, 16>,
}

struct Vertex {
    pos: vec3f,
    look: u32, // axis, and the material << 8
    normal: vec3f,
    tangent_x: f32,
    light: vec3f,
    tangent_y: f32,
    dir: vec3f,
    tangent_z: f32,
}

@group(0) @binding(1) var<uniform> voxels: Voxels;
@group(0) @binding(2) var<storage, read> materials: array<Material>;
@group(0) @binding(3) var<storage, read> texels: array<u32>;
@group(0) @binding(4) var<storage, read> spec_tables: array<f32>;
@group(0) @binding(5) var<storage, read> spots: array<i32>;
@group(1) @binding(0) var<storage, read> vertices: array<Vertex>;
@group(1) @binding(1) var<storage, read> indices: array<u32>;

struct Corner {
    @builtin(position) clip: vec4f,
    @location(0) pos: vec3f,
    @location(1) normal: vec3f,
    @location(2) light: vec3f,
    @location(3) dir: vec3f,
    @location(4) tangent_dir: vec3f,
    // Only to choose among corners, so not corrected for perspective.
    @location(5) @interpolate(linear) weight: vec3f,
    // The three corners' looks, and whether the triangle is plain.
    @location(6) @interpolate(flat) looks: vec4u,
    @location(7) @interpolate(flat) rate: f32,
    @location(8) view_depth: f32,
}

fn axis_of(look: u32) -> u32 {
    return look & 255u;
}

fn material_of(look: u32) -> u32 {
    return (look >> 8u) & 255u;
}

// plain_look in fill.rs: one texture and one axis at every corner, and no spots.
fn is_plain(a: u32, b: u32, c: u32) -> bool {
    let spotted = (materials[material_of(a)].flags & SPOTS) != 0u;
    return axis_of(a) != IN_SEAM && !spotted && a == b && a == c;
}

fn screen_of(clip: vec4f) -> vec2f {
    let ndc = clip.xy / clip.w;
    return vec2f(ndc.x * 0.5 + 0.5, 0.5 - ndc.y * 0.5) * voxels.size;
}

// mip_level in raster.rs, at the centre of a plain triangle, where a plane's
// value is the mean of its corners'. A triangle through the near plane is under
// the eye, so it takes the sharpest level, as the pieces software cuts it into do.
fn plain_rate(clip: array<vec4f, 3>, texel: array<vec2f, 3>) -> f32 {
    if min(clip[0].w, min(clip[1].w, clip[2].w)) < voxels.near {
        return 0.0;
    }
    var s: array<vec2f, 3>;
    var v: array<vec3f, 3>; // iz, u * iz, v * iz
    for (var k = 0; k < 3; k++) {
        s[k] = screen_of(clip[k]);
        let iz = 1.0 / clip[k].w;
        v[k] = vec3f(iz, texel[k] * iz);
    }
    let e = vec4f(s[1] - s[0], s[2] - s[0]);
    let det = e.x * e.w - e.z * e.y;
    let d1 = v[1] - v[0];
    let d2 = v[2] - v[0];
    let gx = (d1 * e.w - d2 * e.y) / det;
    let gy = (d2 * e.x - d1 * e.z) / det;
    let centre = (v[0] + v[1] + v[2]) / 3.0;
    let inv = 1.0 / max(centre.x, 1e-12);
    let uv = centre.yz * inv;
    let a = max(abs((gx.y - uv.x * gx.x) * inv), abs((gx.z - uv.y * gx.x) * inv));
    let b = max(abs((gy.y - uv.x * gy.x) * inv), abs((gy.z - uv.y * gy.x) * inv));
    return max(min(max(a, b), min(a, b) * LONG_AXIS_LIMIT), max(a, b) / LONG_AXIS_LIMIT);
}

// rate_of in blend.rs: a surface face on at its nearest corner.
fn blended_rate(clip: array<vec4f, 3>) -> f32 {
    let nearest = max(min(clip[0].w, min(clip[1].w, clip[2].w)), voxels.near);
    return TEXELS_PER_UNIT * nearest / voxels.focal;
}

fn texel_coords(pos: vec3f, axis: u32) -> vec2f {
    let a = min(axis, 2u) * 2u;
    return vec2f(dot(pos, voxels.axes[a].xyz), dot(pos, voxels.axes[a + 1u].xyz)) * TEXELS_PER_UNIT;
}

// add_dynamic in voxel/fill.rs: `light` and the lights of `mask` at a corner.
fn with_dynamic(light: vec3f, pos: vec3f, normal: vec3f, mask: u32) -> vec3f {
    var out = light;
    var left = mask;
    while left != 0u {
        let i = firstTrailingBit(left);
        left &= left - 1u;
        let l = frame.lights[i];
        let to = l.xyz - pos;
        let d = length(to);
        if d >= l.w || d <= 0.0 {
            continue;
        }
        let fall = 1.0 - d / l.w;
        let lambert = max(dot(normal, to) / d, 0.0);
        out += frame.light_colors[i].rgb * (256.0 * fall * fall * lambert);
    }
    return out;
}

// The instance is the chunk's light mask.
@vertex
fn vs(@builtin(vertex_index) at: u32, @builtin(instance_index) lights: u32) -> Corner {
    let first = at - at % 3u;
    var clip: array<vec4f, 3>;
    var looks: array<u32, 3>;
    var pos: array<vec3f, 3>;
    for (var k = 0u; k < 3u; k++) {
        let v = vertices[indices[first + k]];
        pos[k] = v.pos;
        looks[k] = v.look;
        clip[k] = frame.world_to_clip * vec4f(v.pos, 1.0);
    }
    let plain = is_plain(looks[0], looks[1], looks[2]);
    var rate = blended_rate(clip);
    if plain {
        let axis = axis_of(looks[0]);
        let texel = array(texel_coords(pos[0], axis), texel_coords(pos[1], axis), texel_coords(pos[2], axis));
        rate = plain_rate(clip, texel);
    }
    let corner = at % 3u;
    let me = vertices[indices[at]];
    var weight = vec3f(0.0);
    weight[corner] = 1.0;
    let tangent = vec3f(me.tangent_x, me.tangent_y, me.tangent_z);
    let all = vec4u(looks[0], looks[1], looks[2], u32(plain));
    let here = clip[corner];
    let light = with_dynamic(me.light, me.pos, me.normal, lights);
    return Corner(here, me.pos, me.normal, light, me.dir, tangent, weight, all, rate, here.w);
}

// lerp_texel in texture.rs: two channels at a time, as the CPU does.
fn lerp_texel(a: u32, b: u32, t: u32) -> u32 {
    let rb = ((a & 0xff00ffu) * (256u - t) + (b & 0xff00ffu) * t) >> 8u;
    let g = (((a >> 8u) & 0xff00ffu) * (256u - t) + ((b >> 8u) & 0xff00ffu) * t) >> 8u;
    return 0xff000000u | (rb & 0xff00ffu) | ((g & 0xff00ffu) << 8u);
}

fn heap_texel(base: u32, w: u32, h: u32, x: u32, y: u32) -> u32 {
    return texels[base + (y & (h - 1u)) * w + (x & (w - 1u))];
}

// LodSampler: `level` of `m` at full-size 16.16 coordinates.
fn sample(m: Material, level: u32, f: vec2i) -> vec3f {
    let base = m.levels[level];
    let w = max(m.width >> level, 1u);
    let h = max(m.height >> level, 1u);
    let at = f >> vec2u(level);
    var texel: u32;
    if (m.flags & SMOOTH) == 0u {
        let x = bitcast<vec2u>(f >> vec2u(16u)) >> vec2u(level);
        texel = heap_texel(base, w, h, x.x, x.y);
    } else {
        let p = at - vec2i(0x8000);
        let x0 = bitcast<vec2u>(p >> vec2u(16u));
        let t = bitcast<vec2u>((p >> vec2u(8u)) & vec2i(255));
        let top = lerp_texel(heap_texel(base, w, h, x0.x, x0.y), heap_texel(base, w, h, x0.x + 1u, x0.y), t.x);
        let low = lerp_texel(heap_texel(base, w, h, x0.x, x0.y + 1u), heap_texel(base, w, h, x0.x + 1u, x0.y + 1u), t.x);
        texel = lerp_texel(top, low, t.y);
    }
    return unpack(texel);
}

// NormalSample in bump.rs: the four bytes blended with exact integer weights.
fn normal_at(m: Material, f: vec2i) -> vec3f {
    let p = f - vec2i(0x8000);
    let x0 = bitcast<vec2u>(p >> vec2u(16u));
    let t = bitcast<vec2u>((p >> vec2u(8u)) & vec2i(255));
    let w = vec4u((256u - t.x) * (256u - t.y), t.x * (256u - t.y), (256u - t.x) * t.y, t.x * t.y);
    let c = array(
        heap_texel(m.normals, m.width, m.height, x0.x, x0.y),
        heap_texel(m.normals, m.width, m.height, x0.x + 1u, x0.y),
        heap_texel(m.normals, m.width, m.height, x0.x, x0.y + 1u),
        heap_texel(m.normals, m.width, m.height, x0.x + 1u, x0.y + 1u),
    );
    var sum = vec3u(0u);
    for (var i = 0; i < 4; i++) {
        sum += vec3u((c[i] >> 16u) & 255u, (c[i] >> 8u) & 255u, c[i] & 255u) * w[i];
    }
    return vec3f(sum) * (1.0 / (65536.0 * 127.5)) - vec3f(1.0);
}

// 16.16 texel coordinates of `pos` on `axis`, less whole repeats of the base
// level, which every level's wrap ignores, so that they fit in 32 bits.
fn fixed_coords(pos: vec3f, axis: u32, m: Material) -> vec2i {
    let t = texel_coords(pos, axis);
    let size = vec2f(f32(m.width), f32(m.height));
    return vec2i((t - floor(t / size) * size) * 65536.0);
}

fn level_of(m: Material, rate: f32) -> u32 {
    if m.mips == 0u || !(rate > SHARP_RATE) {
        return 0u;
    }
    return min(u32(ceil(log2(rate / SHARP_RATE))), m.mips);
}

fn bump_scale(rate: f32) -> f32 {
    let t = (BUMP_GONE - rate) / (BUMP_GONE - BUMP_FULL);
    return select(0.0, clamp(t, 0.0, 1.0), t == t);
}

// world_half in voxel/shader.rs.
fn world_half(to_light: vec3f, pos: vec3f) -> vec3f {
    let l = unit_or(to_light, FLAT_NORMAL);
    return unit_or(l + unit_or(voxels.eye - pos, FLAT_NORMAL), l);
}

fn gloss_dot(map: vec3f, h_in: vec3f, s_in: vec3f, axis: u32) -> f32 {
    let h = h_in * (1.5 - 0.5 * dot(h_in, h_in));
    let s = s_in * (1.5 - 0.5 * dot(s_in, s_in));
    let a = min(axis, 2u) * 2u;
    return map.x * dot(h, voxels.axes[a].xyz) - map.y * dot(h, voxels.axes[a + 1u].xyz) + map.z * dot(h, s);
}

fn dominant_axis(n: vec3f) -> u32 {
    let a = abs(n);
    if a.x > a.z && a.x >= a.y {
        return 0u;
    }
    if a.y > a.z && a.y > a.x {
        return 1u;
    }
    return 2u;
}

// seam_of in voxel/mod.rs: the two largest parts and how far the first leads.
fn seam_of(n: vec3f) -> vec3f {
    let a = abs(n);
    let i = dominant_axis(n);
    let p = (i + 1u) % 3u;
    let q = (i + 2u) % 3u;
    let second = select(q, p, a[p] >= a[q]);
    return vec3f(f32(i), f32(second), (a[i] - a[second]) / max(a[i], 1e-6));
}

// pick_corner in blend.rs.
fn pick_corner(w: vec3f, t: f32) -> u32 {
    var first = 0u;
    var second = 1u;
    if w[1] > w[0] {
        first = 1u;
        second = 0u;
    }
    if w[2] > w[first] {
        second = first;
        first = 2u;
    } else if w[2] > w[second] {
        second = 2u;
    }
    let lead = (w[first] - w[second]) / MATERIAL_BAND;
    return select(second, first, lead >= 1.0 || t < 0.5 + 0.5 * lead);
}

fn spot_distance(x: i32, y: i32, z: i32) -> f32 {
    let m = i32(SPOT_EDGE) - 1;
    let edge = i32(SPOT_EDGE);
    return f32(spots[(x & m) + (y & m) * edge + (z & m) * edge * edge]);
}

// Spots::coverage in voxel/solid.rs.
fn spot_cover(p: vec3f, pixel: f32) -> f32 {
    let f = p / SPOT_SPACING;
    let b = floor(f);
    let c = vec3i(b);
    let t = f - b;
    var corner: array<f32, 4>;
    for (var i = 0; i < 4; i++) {
        let dy = i & 1;
        let dz = i >> 1;
        let lo = spot_distance(c.x, c.y + dy, c.z + dz);
        corner[i] = lo + (spot_distance(c.x + 1, c.y + dy, c.z + dz) - lo) * t.x;
    }
    let near = corner[0] + (corner[1] - corner[0]) * t.y;
    let far = corner[2] + (corner[3] - corner[2]) * t.y;
    let d = (near + (far - near) * t.z) * SPOT_BAND / 127.0;
    let edge = clamp(0.5 - d / max(pixel, SPOT_MIN_EDGE), 0.0, 1.0);
    return edge + (SPOT_MEAN - edge) * clamp(2.0 * pixel / SPOT_LOST_AT - 1.0, 0.0, 1.0);
}

fn mul_rgb3(texel: vec3f, k: vec3f, dither: f32) -> vec3f {
    return min(floor(texel * k * (1.0 / 256.0) + vec3f(dither)), vec3f(255.0));
}

fn with_spec(out: vec3f, k: vec3f, s: f32) -> vec3f {
    if s <= MIN_SPEC {
        return out;
    }
    return min(out + floor(k * (s * SPEC_GAIN)), vec3f(255.0));
}

fn shade_plain(c: Corner, dither: f32) -> vec3f {
    let axis = axis_of(c.looks.x);
    let m = materials[material_of(c.looks.x)];
    let f = fixed_coords(c.pos, axis, m);
    let texel = sample(m, level_of(m, c.rate), f);
    var k = c.light;
    var n = FLAT_NORMAL;
    let scale = bump_scale(c.rate);
    if frame.bump_on != 0u && m.normals != NO_NORMALS && scale > 0.0 {
        n = mix(FLAT_NORMAL, normal_at(m, f), scale);
        k *= bump_gain(n, c.tangent_dir);
    }
    k = soft_limit(k);
    var out = mul_rgb3(texel, k, dither);
    if frame.spec_on != 0u && (m.flags & SPEC_VISIBLE) != 0u {
        let h = world_half(c.dir, c.pos);
        let s = spec_response(m.spec_row, gloss_dot(n, h, unit_or(c.normal, FLAT_NORMAL), axis));
        out = with_spec(out, k, s);
    }
    return out;
}

fn shade_blended(c: Corner, at: vec2u, dither: f32) -> vec3f {
    let surface = c.normal;
    let seam = seam_of(surface);
    let axis = u32(seam.x);
    // How much of the second projection shows: half where they meet.
    let fade = select(-1.0, 0.5 * (1.0 - seam.z / SEAM_BAND), seam.z < SEAM_BAND);
    var corner = 0u;
    let one_material = material_of(c.looks.x) == material_of(c.looks.y) && material_of(c.looks.y) == material_of(c.looks.z);
    if !one_material {
        let row = voxels.bayer[at.y & 3u];
        let t = (f32(row[at.x & 3u]) + 0.5) / 16.0;
        corner = pick_corner(c.weight, t);
    }
    let m = materials[material_of(c.looks[corner])];
    let level = level_of(m, c.rate);
    let scale = bump_scale(c.rate);
    let f = fixed_coords(c.pos, axis, m);
    // The two normal maps do not agree either, so relief fades to flat at the line.
    let relief = select(scale, scale * (1.0 - 2.0 * fade), fade >= 0.0);
    var n = FLAT_NORMAL;
    var gain = 1.0;
    if frame.bump_on != 0u && m.normals != NO_NORMALS && scale > 0.0 {
        n = mix(FLAT_NORMAL, normal_at(m, f), min(relief, 1.0));
        let a = axis * 2u;
        let d = c.dir;
        let tangent = vec3f(dot(d, voxels.axes[a].xyz), -dot(d, voxels.axes[a + 1u].xyz), dot(d, surface));
        gain = bump_gain(n, tangent);
    }
    let k = soft_limit(c.light * gain);
    var texel = sample(m, level, f);
    if fade >= 0.0 {
        let other = sample(m, level, fixed_coords(c.pos, u32(seam.y), m));
        texel = lerp_rgb(texel, other, floor(fade * 256.0));
    }
    if (m.flags & SPOTS) != 0u {
        let cover = spot_cover(c.pos, c.view_depth / voxels.focal);
        texel = lerp_rgb(texel, unpack(SPOT_COLOR), floor(cover * 256.0));
    }
    var out = mul_rgb3(texel, k, dither);
    if frame.spec_on != 0u && (m.flags & SPEC_VISIBLE) != 0u {
        let h = world_half(c.dir, c.pos);
        let s = spec_response(m.spec_row, gloss_dot(n, h, unit_or(surface, FLAT_NORMAL), axis));
        out = with_spec(out, k, s);
    }
    return out;
}

@fragment
fn fs(c: Corner) -> @location(0) vec4f {
    let dither = dither_of(c.clip.xy);
    var out: vec3f;
    if c.looks.w != 0u {
        out = shade_plain(c, dither);
    } else {
        out = shade_blended(c, vec2u(c.clip.xy), dither);
    }
    return finished(with_fog(out, c.view_depth));
}
