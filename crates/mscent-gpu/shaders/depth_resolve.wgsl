// A multisampled frame's depth as one value a pixel, for reading back: the mean
// of the samples that hit something. Depth is linear in 1/z, which is linear
// across the screen, and the samples sit around the pixel's centre, so on a
// surface the mean is the centre's depth, which is what software stores. Where no
// sample hit anything it is 0.0, nothing.

@group(0) @binding(0) var depth: texture_depth_multisampled_2d;

@vertex
fn vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4f {
    let corners = array(vec2f(-1.0, -1.0), vec2f(3.0, -1.0), vec2f(-1.0, 3.0));
    return vec4f(corners[i], 0.0, 1.0);
}

@fragment
fn fs(@builtin(position) at: vec4f) -> @location(0) vec4f {
    let pixel = vec2u(at.xy);
    var sum = 0.0;
    var hits = 0.0;
    for (var s = 0u; s < textureNumSamples(depth); s++) {
        let d = textureLoad(depth, pixel, i32(s));
        sum += d;
        hits += select(0.0, 1.0, d > 0.0);
    }
    return vec4f(select(0.0, sum / max(hits, 1.0), hits > 0.0), 0.0, 0.0, 0.0);
}
