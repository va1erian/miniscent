//! A play room whose far wall, at y = 48, is a viewport onto a room at scale 4
//! that continues it, and how to read the baked light of a floor: for the tests
//! of light crossing a viewport, either way.

use std::path::Path;

use mscent_math::{Vec3, Vec3d};
use mscent_render::{BuildConfig, LevelRenderer, MaterialTable};
use mscent_world::materials::{METAL, VIEW_FIRST};
use mscent_world::{Brush, Entity, Level, MAX_VISTA_FOG, Op};

/// Where the layer's room is.
pub const AWAY: f32 = 1000.0;
/// The layer's scale is 4.
pub const PARALLAX: f32 = 0.25;
/// The play floor's height, which the room's floor continues.
pub const FLOOR: f32 = -16.0;

pub fn materials() -> MaterialTable {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/textures");
    MaterialTable::load_dir(&dir).expect("textures")
}

pub fn cuboid(min: [f32; 3], max: [f32; 3], op: Op, order: u32, material: u16) -> Brush {
    let v = |p: [f32; 3]| Vec3d::new(f64::from(p[0]), f64::from(p[1]), f64::from(p[2]));
    Brush::cuboid(v(min), v(max), op, order)
        .expect("a cuboid")
        .with_material(material)
}

pub fn arr(v: Vec3) -> [f32; 3] {
    [v.x, v.y, v.z]
}

/// Where a play-space point is seen in the layer's room.
pub fn image(p: Vec3) -> Vec3 {
    Vec3::new(AWAY, 0.0, 0.0) + p * PARALLAX
}

/// A play room 32 by 64 whose far wall, at y = 48, is a viewport; a room at
/// scale 4 whose floor continues the play floor, reaching 112 units past it.
pub fn brushes() -> Vec<Brush> {
    let room_lo = image(Vec3::new(-48.0, -32.0, FLOOR));
    let room_hi = image(Vec3::new(48.0, 160.0, 32.0));
    vec![
        cuboid(
            [-16.0, -16.0, FLOOR],
            [16.0, 56.0, 16.0],
            Op::Subtract,
            0,
            METAL,
        ),
        cuboid(
            [-16.0, 48.0, FLOOR],
            [16.0, 56.0, 16.0],
            Op::Add,
            1,
            VIEW_FIRST,
        ),
        cuboid(arr(room_lo), arr(room_hi), Op::Subtract, 2, METAL),
    ]
}

pub fn lamp(pos: Vec3) -> Entity {
    lamp_of(pos, 40.0)
}

pub fn lamp_of(pos: Vec3, radius: f32) -> Entity {
    Entity::Light {
        pos,
        color: Vec3::new(1.0, 0.9, 0.8),
        radius,
        intensity: 1.0,
    }
}

pub fn layer(parallax: f32) -> Entity {
    layer_at(parallax, 0)
}

/// A layer of slot 0 drawn `index`-th, showing the one room.
pub fn layer_at(parallax: f32, index: u8) -> Entity {
    Entity::VistaLayer {
        pos: Vec3::new(AWAY, 0.0, 0.0),
        centre: Vec3::ZERO,
        slot: 0,
        layer: index,
        parallax,
        spin: Vec3::ZERO,
        pan: [0.0, 0.0],
        fog_start: 0.0,
        fog_end: MAX_VISTA_FOG,
        fog_color: 0,
    }
}

pub fn renderer(extra: Vec<Brush>, mut entities: Vec<Entity>) -> LevelRenderer {
    entities.push(Entity::PlayerStart {
        pos: Vec3::ZERO,
        yaw: 0.0,
    });
    let mut all = brushes();
    all.extend(extra);
    let level = Level {
        brushes: all,
        entities,
        ..Level::default()
    };
    let cfg = BuildConfig {
        luxel_size: 0.125,
        shadow_samples: 1,
        light_radius: 0.0,
        ..BuildConfig::default()
    };
    LevelRenderer::from_level(&level, materials(), &cfg).expect("builds")
}

/// The baked light of the floor face under `p`, as linear RGB.
pub fn floor_light(r: &LevelRenderer, p: Vec3) -> Vec3 {
    let faces = r.scene().faces();
    let on = |i: &usize| {
        let f = &faces[*i];
        let n = f.plane.normal.to_f32();
        let above = f.plane.signed_distance(p.to_f64()).abs() < 1e-3;
        let (lo, hi) = bounds(f.polygon.vertices());
        let inside = (lo.x..=hi.x).contains(&p.x) && (lo.y..=hi.y).contains(&p.y);
        n.z > 0.99 && above && inside
    };
    let face = (0..faces.len()).find(on).expect("a floor under the point");
    r.lightmap()
        .sample_world(face, p)
        .expect("a luxel there")
        .color
}

pub fn bounds(verts: &[Vec3d]) -> (Vec3, Vec3) {
    let pts = verts.iter().map(|v| v.to_f32());
    let pick =
        |a: Vec3, b: Vec3, f: fn(f32, f32) -> f32| Vec3::new(f(a.x, b.x), f(a.y, b.y), f(a.z, b.z));
    let lo = pts
        .clone()
        .fold(Vec3::new(f32::MAX, f32::MAX, f32::MAX), |a, b| {
            pick(a, b, f32::min)
        });
    let hi = pts.fold(Vec3::new(f32::MIN, f32::MIN, f32::MIN), |a, b| {
        pick(a, b, f32::max)
    });
    (lo, hi)
}

pub fn luminance(c: Vec3) -> f32 {
    (c.x + c.y + c.z) / 3.0
}
