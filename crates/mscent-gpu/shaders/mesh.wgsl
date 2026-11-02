// Actors: a flat colour under a light factor per corner, as GouraudShader does.

override MAX_LIGHT: f32;

struct Corner {
    @builtin(position) clip: vec4f,
    @location(0) @interpolate(flat) color: u32,
    @location(1) light: f32,
    @location(2) view_depth: f32,
}

@vertex
fn vs(@location(0) pos: vec3f, @location(1) color: u32, @location(2) light: f32) -> Corner {
    let clip = frame.world_to_clip * vec4f(pos, 1.0);
    return Corner(clip, color, light, clip.w);
}

@fragment
fn fs(corner: Corner) -> @location(0) vec4f {
    let k = clamp(corner.light * 256.0, 0.0, MAX_LIGHT * 256.0);
    return finished(with_fog(mul_rgb(unpack(corner.color), k), corner.view_depth));
}
