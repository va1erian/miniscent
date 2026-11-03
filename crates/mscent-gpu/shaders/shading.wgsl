// The light maths that faces and voxels share: bump, the knee and the
// highlight's table. A shader that includes this declares `spec_tables`, the
// tables `SpecTable::response` reads, at a binding of its own.

override MIN_FLAT: f32;
override MIN_GAIN: f32;
override MAX_GAIN: f32;
override RELIEF: f32;
override KNEE: f32;
override ROLL: f32;
override TABLE_LEN: u32;

fn unit_or(v: vec3f, fallback: vec3f) -> vec3f {
    let length_squared = dot(v, v);
    return select(fallback, v / sqrt(length_squared), length_squared > 0.0);
}

fn bump_gain(n: vec3f, l: vec3f) -> f32 {
    let flat_response = max(l.z, MIN_FLAT);
    let tilted = n.x * RELIEF * l.x + n.y * RELIEF * l.y + n.z * flat_response;
    return clamp(tilted / flat_response, MIN_GAIN, MAX_GAIN);
}

// The identity below the knee, so it needs no test of the brightest channel.
fn soft_limit(k: vec3f) -> vec3f {
    let d = max(k - vec3f(KNEE), vec3f(0.0));
    return k - d * d / (vec3f(ROLL) + d);
}

// SpecTable::response, for the table of `row`.
fn spec_response(row: u32, n_dot_h: f32) -> f32 {
    let x = clamp(n_dot_h, 0.0, 1.0) * f32(TABLE_LEN - 1u);
    let i = min(u32(x), TABLE_LEN - 2u);
    let first = spec_tables[row * TABLE_LEN + i];
    return first + (spec_tables[row * TABLE_LEN + i + 1u] - first) * (x - f32(i));
}
