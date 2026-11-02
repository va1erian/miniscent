// A level's faces in one flat colour each, worked out on the CPU.

struct Frame {
    world_to_clip: mat4x4f,
}

@group(0) @binding(0) var<uniform> frame: Frame;

struct Corner {
    @builtin(position) clip: vec4f,
    @location(0) color: vec4f,
}

// Colour arrives as the bytes of 0xAARRGGBB: blue first.
@vertex
fn vs(@location(0) pos: vec3f, @location(1) color: vec4f) -> Corner {
    return Corner(frame.world_to_clip * vec4f(pos, 1.0), color.zyxw);
}

@fragment
fn fs(corner: Corner) -> @location(0) vec4f {
    return corner.color;
}
