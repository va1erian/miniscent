// The sky gradient, in the integers of Framebuffer::clear_sky and lerp_rgb, so
// it is the same bytes rather than close to them.

struct Sky {
    top: u32,
    bottom: u32,
    last_row: u32,
}

@group(0) @binding(0) var<uniform> sky: Sky;

@vertex
fn vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4f {
    let corners = array(vec2f(-1.0, -1.0), vec2f(3.0, -1.0), vec2f(-1.0, 3.0));
    return vec4f(corners[i], 0.0, 1.0);
}

fn channel(shift: u32, t: i32) -> f32 {
    let from_top = i32((sky.top >> shift) & 255u);
    let to_bottom = i32((sky.bottom >> shift) & 255u);
    return f32(from_top + (((to_bottom - from_top) * t) >> 8u));
}

@fragment
fn fs(@builtin(position) at: vec4f) -> @location(0) vec4f {
    let t = i32(min(u32(at.y) * 256u / sky.last_row, 256u));
    return vec4f(channel(16u, t), channel(8u, t), channel(0u, t), 255.0) / 255.0;
}
