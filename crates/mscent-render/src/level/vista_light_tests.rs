//! Each reason a lamp is kept from a vista, alone: every case is set beside one
//! that differs only in that reason and is carried.

use mscent_math::{Vec3, Vec3d};
use mscent_world::lightmap::{Cone, PointLight};
use mscent_world::materials::{METAL, VIEW_FIRST};
use mscent_world::{Brush, Bsp, Face, Op, compile, extract_faces};

use super::{MAX_CARRY_RAYS, shines_on};

fn cuboid(lo: [f64; 3], hi: [f64; 3], op: Op, order: u32, material: u16) -> Brush {
    let v = |p: [f64; 3]| Vec3d::new(p[0], p[1], p[2]);
    Brush::cuboid(v(lo), v(hi), op, order)
        .expect("a cuboid")
        .with_material(material)
}

/// A room 32 by 72 whose far wall, at y = 48, is a viewport, and `extra`.
fn scene(extra: Vec<Brush>) -> Vec<Brush> {
    let mut all = vec![
        cuboid(
            [-16.0, -16.0, -16.0],
            [16.0, 56.0, 16.0],
            Op::Subtract,
            0,
            METAL,
        ),
        cuboid(
            [-16.0, 48.0, -16.0],
            [16.0, 56.0, 16.0],
            Op::Add,
            1,
            VIEW_FIRST,
        ),
    ];
    all.extend(extra);
    all
}

/// The viewport face of `scene`.
fn window() -> Face {
    let brushes = scene(Vec::new());
    let bsp = compile(&brushes).expect("compiles");
    (extract_faces(&bsp, &brushes).into_iter())
        .find(|f| f.material == VIEW_FIRST && f.plane.normal.y < -0.99)
        .expect("the viewport face")
}

/// The room of `scene` without its viewport wall: behind the window is open.
fn open_room() -> Bsp {
    let room = cuboid(
        [-16.0, -16.0, -16.0],
        [16.0, 56.0, 16.0],
        Op::Subtract,
        0,
        METAL,
    );
    compile(&[room]).expect("compiles")
}

fn lamp(position: Vec3, radius: f32) -> PointLight {
    PointLight {
        position,
        color: Vec3::new(1.0, 1.0, 1.0),
        radius,
        cone: None,
    }
}

fn shines(bsp: &Bsp, lamp: &PointLight) -> bool {
    shines_on(bsp, lamp, &window(), &mut MAX_CARRY_RAYS.clone())
}

#[test]
fn a_lamp_behind_the_window_plane_is_kept_out_with_a_clear_sight_line() {
    // Nothing stands between the two lamps and the window: only the plane does.
    let bsp = open_room();
    assert!(shines(&bsp, &lamp(Vec3::new(0.0, 40.0, 0.0), 40.0)));
    assert!(!shines(&bsp, &lamp(Vec3::new(0.0, 52.0, 0.0), 40.0)));
}

#[test]
fn a_lamp_hidden_from_the_window_is_kept_out() {
    let wall = cuboid([-16.0, 30.0, -16.0], [16.0, 32.0, 16.0], Op::Add, 2, METAL);
    let brushes = scene(vec![wall]);
    let bsp = compile(&brushes).expect("compiles");
    assert!(shines(&bsp, &lamp(Vec3::new(0.0, 40.0, 0.0), 40.0)));
    assert!(!shines(&bsp, &lamp(Vec3::new(0.0, 20.0, 0.0), 40.0)));
}

#[test]
fn a_projector_aimed_away_from_the_window_is_kept_out() {
    let bsp = open_room();
    let aimed = |dir: Vec3| PointLight {
        cone: Cone::new(dir, 0.3, 0.5),
        ..lamp(Vec3::new(0.0, 30.0, 0.0), 40.0)
    };
    assert!(shines(&bsp, &aimed(Vec3::new(0.0, 1.0, 0.0))));
    assert!(!shines(&bsp, &aimed(Vec3::new(0.0, -1.0, 0.0))));
}

#[test]
fn a_lamp_out_of_range_of_the_window_is_kept_out() {
    // The window is 48 units from the lamp at its nearest.
    let bsp = open_room();
    assert!(shines(&bsp, &lamp(Vec3::ZERO, 48.5)));
    assert!(!shines(&bsp, &lamp(Vec3::ZERO, 47.5)));
}

#[test]
fn a_spent_ray_budget_carries_nothing() {
    let bsp = open_room();
    let near = lamp(Vec3::new(0.0, 40.0, 0.0), 40.0);
    assert!(!shines_on(&bsp, &near, &window(), &mut 0));
    let mut one = 1;
    assert!(shines_on(&bsp, &near, &window(), &mut one));
    assert_eq!(one, 0, "the ray was spent");
}
