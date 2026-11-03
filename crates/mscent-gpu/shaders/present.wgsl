// The game's frame on the window. Each frame pixel is what software leaves in
// its framebuffer: the scene, the white flash as `lerp_toward` blends it, then
// the 2D layer as `draw2d` would have drawn it there. The frame is scaled into
// its letterbox as the platform's `blit_fit` does: by whole pixels at a whole
// factor, else bilinear with centres aligned, and black around it. A layer
// smaller than the scene, its 2D drawn at a lower resolution, goes over the
// scaled scene by whole pixels instead.

// The window's format stores sRGB, so what is written is linear first.
override SRGB: bool;

struct Show {
    rect: vec4f,  // where the frame lands in the window: x, y, width, height
    size: vec2f,  // the frame's, in pixels
    white: f32,   // the flash, 0..256
    whole: u32,   // the scale is a whole factor
    layer_size: vec2f,
    pad: vec2f,
}

@group(0) @binding(0) var<uniform> show: Show;
@group(0) @binding(1) var scene: texture_2d<f32>;
@group(0) @binding(2) var layer: texture_2d<f32>;

@vertex
fn vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4f {
    let corners = array(vec2f(-1.0, -1.0), vec2f(3.0, -1.0), vec2f(-1.0, 3.0));
    return vec4f(corners[i], 0.0, 1.0);
}

// The scene's pixel at `at` with the flash over it, in levels of 255.
fn flashed(at: vec2u) -> vec3f {
    let c = round(textureLoad(scene, at, 0).rgb * 255.0);
    // lerp_rgb toward white, which is never below the colour.
    return c + floor((vec3f(255.0) - c) * show.white / 256.0);
}

// `c` with the layer's pixel at `at` over it.
fn layered(c: vec3f, at: vec2u) -> vec3f {
    let l = textureLoad(layer, at, 0);
    let keep = 255.0 - round(l.a * 255.0);
    return min(round(l.rgb * 255.0) + floor((c * keep + vec3f(127.0)) / 255.0), vec3f(255.0));
}

// The frame's pixel at `at`, the layer the scene's size.
fn pixel(at: vec2u) -> vec3f {
    return layered(flashed(at), at);
}

// `tap` in scale.rs: the lower source index along one axis and the weight of
// the upper one, out of 256.
fn tap(d: f32, src: f32, dst: f32) -> vec2f {
    let x = clamp((d + 0.5) * src / dst - 0.5, 0.0, src - 1.0);
    let i = floor(x);
    return vec2f(i, floor((x - i) * 256.0));
}

// `lerp` in scale.rs, a channel at a time.
fn mix256(a: vec3f, b: vec3f, t: f32) -> vec3f {
    return floor((a * (256.0 - t) + b * t) / 256.0);
}

fn to_window(c: vec3f) -> vec4f {
    var v = c / 255.0;
    if SRGB {
        v = select(pow((v + 0.055) / 1.055, vec3f(2.4)), v / 12.92, v <= vec3f(0.04045));
    }
    return vec4f(v, 1.0);
}

@fragment
fn fs(@builtin(position) at: vec4f) -> @location(0) vec4f {
    let d = floor(at.xy) - show.rect.xy;
    if any(d < vec2f(0.0)) || any(d >= show.rect.zw) {
        return to_window(vec3f(0.0));
    }
    if all(show.layer_size == show.size) {
        return to_window(composed(d));
    }
    let last_layer = vec2u(show.layer_size) - vec2u(1u);
    let at_layer = min(vec2u(d * show.layer_size / show.rect.zw), last_layer);
    return to_window(layered(scaled(d), at_layer));
}

// The scene and the layer of its size, composed and then scaled, as software
// composes its frame and the platform scales it.
fn composed(d: vec2f) -> vec3f {
    let last = vec2u(show.size) - vec2u(1u);
    if show.whole != 0u {
        return pixel(min(vec2u(d * show.size / show.rect.zw), last));
    }
    let tx = tap(d.x, show.size.x, show.rect.z);
    let ty = tap(d.y, show.size.y, show.rect.w);
    let x0 = u32(tx.x);
    let y0 = u32(ty.x);
    let x1 = min(x0 + 1u, last.x);
    let y1 = min(y0 + 1u, last.y);
    let top = mix256(pixel(vec2u(x0, y0)), pixel(vec2u(x1, y0)), tx.y);
    let low = mix256(pixel(vec2u(x0, y1)), pixel(vec2u(x1, y1)), tx.y);
    return mix256(top, low, ty.y);
}

// The flashed scene alone, scaled.
fn scaled(d: vec2f) -> vec3f {
    let last = vec2u(show.size) - vec2u(1u);
    if show.whole != 0u {
        return flashed(min(vec2u(d * show.size / show.rect.zw), last));
    }
    let tx = tap(d.x, show.size.x, show.rect.z);
    let ty = tap(d.y, show.size.y, show.rect.w);
    let x0 = u32(tx.x);
    let y0 = u32(ty.x);
    let x1 = min(x0 + 1u, last.x);
    let y1 = min(y0 + 1u, last.y);
    let top = mix256(flashed(vec2u(x0, y0)), flashed(vec2u(x1, y0)), tx.y);
    let low = mix256(flashed(vec2u(x0, y1)), flashed(vec2u(x1, y1)), tx.y);
    return mix256(top, low, ty.y);
}
