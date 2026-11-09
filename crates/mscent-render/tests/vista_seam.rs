//! A miniature that continues the play space is lit by the play space's lamps,
//! so the two meet at the same brightness: judged on baked luxels at twin
//! points, one in the play room and its image in the layer's room.

use std::path::Path;

use mscent_math::{Vec3, Vec3d};
use mscent_render::{BuildConfig, LevelRenderer, MaterialTable};
use mscent_world::materials::{METAL, VIEW_FIRST};
use mscent_world::{Brush, Entity, Level, MAX_VISTA_FOG, Op};

/// Where the layer's room is.
const AWAY: f32 = 1000.0;
/// The layer's scale is 4.
const PARALLAX: f32 = 0.25;
/// The play floor's height, which the room's floor continues.
const FLOOR: f32 = -16.0;

fn materials() -> MaterialTable {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/textures");
    MaterialTable::load_dir(&dir).expect("textures")
}

fn cuboid(min: [f32; 3], max: [f32; 3], op: Op, order: u32, material: u16) -> Brush {
    let v = |p: [f32; 3]| Vec3d::new(f64::from(p[0]), f64::from(p[1]), f64::from(p[2]));
    Brush::cuboid(v(min), v(max), op, order)
        .expect("a cuboid")
        .with_material(material)
}

fn arr(v: Vec3) -> [f32; 3] {
    [v.x, v.y, v.z]
}

/// Where a play-space point is seen in the layer's room.
fn image(p: Vec3) -> Vec3 {
    Vec3::new(AWAY, 0.0, 0.0) + p * PARALLAX
}

/// A play room 32 by 64 whose far wall, at y = 48, is a viewport; a room at
/// scale 4 whose floor continues the play floor, reaching 112 units past it.
fn brushes() -> Vec<Brush> {
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

fn lamp(pos: Vec3) -> Entity {
    Entity::Light {
        pos,
        color: Vec3::new(1.0, 0.9, 0.8),
        radius: 40.0,
        intensity: 1.0,
    }
}

fn layer(parallax: f32) -> Entity {
    layer_at(parallax, 0)
}

/// A layer of slot 0 drawn `index`-th, showing the one room.
fn layer_at(parallax: f32, index: u8) -> Entity {
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

fn renderer(extra: Vec<Brush>, mut entities: Vec<Entity>) -> LevelRenderer {
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
fn floor_light(r: &LevelRenderer, p: Vec3) -> Vec3 {
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

fn bounds(verts: &[Vec3d]) -> (Vec3, Vec3) {
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

fn luminance(c: Vec3) -> f32 {
    (c.x + c.y + c.z) / 3.0
}

fn near_lamp() -> Entity {
    lamp(Vec3::new(0.0, 40.0, 0.0))
}

#[test]
fn the_floor_beyond_a_viewport_is_lit_as_its_twin_in_the_play_space() {
    let r = renderer(Vec::new(), vec![near_lamp(), layer(PARALLAX)]);
    for y in [24.0, 32.0, 40.0, 44.0, 47.0] {
        for x in [-8.0, 0.0, 6.0] {
            let p = Vec3::new(x, y, FLOOR);
            let (play, view) = (floor_light(&r, p), floor_light(&r, image(p)));
            let (a, b) = (luminance(play), luminance(view));
            println!("({x}, {y}): play {a:.4}, vista {b:.4}");
            assert!(a > 0.05, "the lamp lights the play floor at ({x}, {y})");
            assert!((a - b).abs() <= 0.12 * a, "({x}, {y}): {a} against {b}");
        }
    }
}

#[test]
fn the_floor_past_the_viewport_wall_takes_the_lamps_light() {
    let lit = renderer(Vec::new(), vec![near_lamp(), layer(PARALLAX)]);
    let dark = renderer(Vec::new(), vec![layer(PARALLAX)]);
    // Past the wall at y = 48: only the image of the lamp can light it.
    for y in [49.0, 52.0, 58.0] {
        let p = image(Vec3::new(0.0, y, FLOOR));
        let (a, b) = (
            luminance(floor_light(&lit, p)),
            luminance(floor_light(&dark, p)),
        );
        println!("y {y}: with the lamp {a:.4}, without {b:.4}");
        assert!(a > b * 3.0, "y {y}: {a} against {b}");
    }
}

#[test]
fn a_lamp_behind_the_viewport_wall_lights_nothing_of_the_vista() {
    // A room behind the viewport, with a lamp whose image would fall in the view.
    let behind = cuboid(
        [-16.0, 60.0, FLOOR],
        [16.0, 90.0, 16.0],
        Op::Subtract,
        3,
        METAL,
    );
    let back_lamp = lamp(Vec3::new(0.0, 64.0, 0.0));
    let with = renderer(vec![behind.clone()], vec![back_lamp, layer(PARALLAX)]);
    let without = renderer(vec![behind], vec![layer(PARALLAX)]);
    for y in [50.0, 60.0, 70.0] {
        let p = image(Vec3::new(0.0, y, FLOOR));
        assert_eq!(floor_light(&with, p), floor_light(&without, p), "y {y}");
    }
}

#[test]
fn an_infinitely_far_layer_takes_no_play_lamp() {
    let with = renderer(Vec::new(), vec![near_lamp(), layer(0.0)]);
    let without = renderer(Vec::new(), vec![layer(0.0)]);
    for y in [40.0, 52.0] {
        let p = image(Vec3::new(0.0, y, FLOOR));
        assert_eq!(floor_light(&with, p), floor_light(&without, p), "y {y}");
    }
}

#[test]
fn a_room_shown_by_an_infinite_layer_first_is_lit_by_the_finite_one() {
    // The first layer cannot carry a lamp; the room still takes the second's.
    let shown = vec![layer_at(0.0, 0), layer_at(PARALLAX, 1)];
    let lit = renderer(Vec::new(), [vec![near_lamp()], shown.clone()].concat());
    let dark = renderer(Vec::new(), shown);
    let p = image(Vec3::new(0.0, 52.0, FLOOR));
    let (a, b) = (
        luminance(floor_light(&lit, p)),
        luminance(floor_light(&dark, p)),
    );
    assert!(a > b * 3.0, "{a} against {b}");
}
