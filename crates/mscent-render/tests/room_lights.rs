//! A dynamic light lights the rooms its room's row of the level's table reaches
//! (`scene/reach.rs`), read from real frames: a light beside a wall leaves the
//! room behind it dark, a light in a doorway lights both rooms, and a light that
//! moves from one room to the other lights the other, with no flood in between.

use std::path::Path;

use mscent_math::{Vec3, Vec3d};
use mscent_render::{
    BuildConfig, DynLight, Effects, Framebuffer, LevelRenderer, MaterialTable, Quality, View,
};
use mscent_world::{Brush, Level, Op};

fn cuboid(a: [f64; 3], b: [f64; 3], order: u32) -> Brush {
    let v = |p: [f64; 3]| Vec3d::new(p[0], p[1], p[2]);
    Brush::cuboid(v(a), v(b), Op::Subtract, order).expect("a brush")
}

fn renderer(brushes: Vec<Brush>) -> LevelRenderer {
    let level = Level {
        brushes,
        entities: Vec::new(),
        labels: Vec::new(),
        mover_groups: Vec::new(),
        volumes: Vec::new(),
    };
    let cfg = BuildConfig {
        luxel_size: 1.0,
        shadow_samples: 1,
        ..BuildConfig::default()
    };
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/textures");
    let materials = MaterialTable::load_dir(&dir).expect("textures");
    LevelRenderer::from_level(&level, materials, &cfg).expect("a level")
}

const WEST: [[f64; 3]; 2] = [[-20.0, -10.0, -10.0], [0.0, 10.0, 10.0]];
const EAST: [[f64; 3]; 2] = [[4.0, -10.0, -10.0], [24.0, 10.0, 10.0]];

/// Two rooms with a wall 4 thick between them and no way through but a U of
/// corridor that goes 100 up and comes down again.
fn walled() -> LevelRenderer {
    renderer(vec![
        cuboid(WEST[0], WEST[1], 0),
        cuboid(EAST[0], EAST[1], 1),
        cuboid([-12.0, -2.0, 9.0], [-8.0, 2.0, 100.0], 2),
        cuboid([-12.0, -2.0, 96.0], [16.0, 2.0, 100.0], 3),
        cuboid([12.0, -2.0, 9.0], [16.0, 2.0, 100.0], 4),
    ])
}

/// The same rooms with a doorway through the wall.
fn doorway() -> LevelRenderer {
    renderer(vec![
        cuboid(WEST[0], WEST[1], 0),
        cuboid(EAST[0], EAST[1], 1),
        cuboid([0.0, -3.0, -3.0], [4.0, 3.0, 3.0], 2),
    ])
}

fn v(x: f32, y: f32, z: f32) -> Vec3 {
    Vec3::new(x, y, z)
}

/// An eye in the west room and one in the east, both looking down at the floor
/// by the wall.
fn west_view() -> View {
    View::from_angles(v(-14.0, 0.0, 6.0), -1.2, -0.6, 0.0)
}

fn east_view() -> View {
    View::from_angles(v(14.0, 0.0, 6.0), 1.2, -0.6, 0.0)
}

fn frame(r: &LevelRenderer, view: &View, lights: &[DynLight]) -> Vec<u32> {
    let mut fb = Framebuffer::new(160, 90).expect("a target");
    let effects = Effects {
        lights,
        ..Effects::default()
    };
    r.render_with_effects(&mut fb, view, Quality::Medium, &[], None, &effects)
        .expect("a frame");
    fb.pixels().to_vec()
}

fn light_at(r: &LevelRenderer, pos: Vec3) -> DynLight {
    let mut light = DynLight::new();
    assert!(light.place(r.scene(), pos, 20.0, v(2.0, 2.0, 2.0)));
    light
}

/// How many pixels the lights change.
fn changed(r: &LevelRenderer, view: &View, lights: &[DynLight]) -> usize {
    let (dark, lit) = (frame(r, view, &[]), frame(r, view, lights));
    dark.iter().zip(&lit).filter(|(a, b)| a != b).count()
}

#[test]
fn a_light_beside_a_wall_leaves_the_far_room_dark() {
    let r = walled();
    // Close to the floor and 2 from a wall that is 4 thick: the floor of the east
    // room under it is well within the light's 20.
    let light = [light_at(&r, v(-2.0, 0.0, -8.0))];
    assert!(
        changed(&r, &west_view(), &light) > 1000,
        "it lights its own room"
    );
    assert_eq!(
        changed(&r, &east_view(), &light),
        0,
        "and nothing behind the wall"
    );
    let east = r.scene().bsp().leaf_at_f32(v(14.0, 0.0, 0.0));
    assert!(!light[0].reaches_leaf(east));
}

#[test]
fn a_light_in_a_doorway_lights_both_rooms() {
    let r = doorway();
    let light = [light_at(&r, v(2.0, 0.0, -2.5))];
    assert!(changed(&r, &west_view(), &light) > 1000, "west");
    assert!(changed(&r, &east_view(), &light) > 1000, "east");
}

#[test]
fn a_light_that_moves_to_the_other_room_lights_that_room_and_not_this_one() {
    let r = walled();
    let mut light = [DynLight::new()];
    let white = v(2.0, 2.0, 2.0);
    assert!(light[0].place(r.scene(), v(-2.0, 0.0, -8.0), 20.0, white));
    let (west_lit, east_lit) = (
        changed(&r, &west_view(), &light),
        changed(&r, &east_view(), &light),
    );
    assert!(west_lit > 1000 && east_lit == 0, "{west_lit} {east_lit}");
    // The same light, put down on the other side of the wall.
    assert!(light[0].place(r.scene(), v(6.0, 0.0, -8.0), 20.0, white));
    let (west_lit, east_lit) = (
        changed(&r, &west_view(), &light),
        changed(&r, &east_view(), &light),
    );
    assert!(west_lit == 0 && east_lit > 1000, "{west_lit} {east_lit}");
}
