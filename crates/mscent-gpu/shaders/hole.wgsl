// A viewport face: a wall at its true depth that shows the picture of its slot's
// vista, drawn beforehand into one layer of `vistas` (scene/holes.rs and
// Framebuffer::open_view in software).

@group(0) @binding(1) var vistas: texture_2d_array<f32>;

struct Corner {
    @builtin(position) clip: vec4f,
    @location(0) @interpolate(flat) layer: u32,
}

@vertex
fn vs(@location(0) pos: vec3f, @location(1) layer: u32) -> Corner {
    return Corner(frame.world_to_clip * vec4f(pos, 1.0), layer);
}

@fragment
fn fs(corner: Corner) -> @location(0) vec4f {
    return textureLoad(vistas, vec2i(corner.clip.xy), corner.layer, 0);
}
