// What stands in a light's way, drawn into one face of its cube map as the distance
// from the light over its radius. Distance and not projected depth, so that the
// receiver's lookup needs no per-face matrix: it only has to know which way it is
// from the light (shadow_sample.wgsl). A face's depth buffer sorts the nearest.

override SHADOW_NEAR: f32;

// The light, then the face's own axes: right and up on the image, and forward.
struct Cube {
    light: vec4f, // position, and radius
    right: vec4f,
    up: vec4f,
    forward: vec4f,
}

struct Pose {
    to_world: mat4x4f,
    eye: vec4f,
    pan: vec4f,
}

// A model's vertex, as voxel.wgsl has it: only the position is read here.
struct Vertex {
    pos: vec3f,
    look: u32,
    normal: vec3f,
    tangent_x: f32,
    light: vec3f,
    tangent_y: f32,
    dir: vec3f,
    tangent_z: f32,
}

@group(0) @binding(0) var<uniform> cube: Cube;
@group(1) @binding(0) var<storage, read> vertices: array<Vertex>;
@group(1) @binding(1) var<storage, read> corners: array<u32>;
@group(1) @binding(2) var<storage, read> worlds: array<mat4x4f>;
@group(1) @binding(3) var<storage, read> poses: array<Pose>;

struct Caster {
    @builtin(position) clip: vec4f,
    @location(0) world: vec3f,
}

// A 90 degree view down the face's axis, with depth running from 0 at the near
// plane to 1 at the light's reach.
fn project(world: vec3f) -> Caster {
    let v = world - cube.light.xyz;
    let reach = cube.light.w;
    let along = dot(v, cube.forward.xyz);
    let slope = reach / (reach - SHADOW_NEAR);
    let clip = vec4f(dot(v, cube.right.xyz), dot(v, cube.up.xyz), slope * (along - SHADOW_NEAR), along);
    return Caster(clip, world);
}

// Actors: their triangles are already in the world.
@vertex
fn vs_world(@location(0) pos: vec3f) -> Caster {
    return project(pos);
}

// Enemy models: pulled as the voxel pipeline pulls them, one instance a part.
@vertex
fn vs_model(@builtin(vertex_index) at: u32, @builtin(instance_index) part: u32) -> Caster {
    let pos = vertices[corners[at]].pos;
    return project((worlds[part] * vec4f(pos, 1.0)).xyz);
}

// Movers: a face's vertices as authored, one instance a pose.
@vertex
fn vs_mover(@location(0) pos: vec3f, @builtin(instance_index) pose: u32) -> Caster {
    return project((poses[pose].to_world * vec4f(pos, 1.0)).xyz);
}

@fragment
fn fs(caster: Caster) -> @location(0) f32 {
    return length(caster.world - cube.light.xyz) / cube.light.w;
}
