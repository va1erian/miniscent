use super::*;
use mscent_math::Vec3d;
use mscent_world::{Brush, Op, Portals, compile, extract_faces};

use crate::texture::MaterialTable;

fn cuboid(a: [f64; 3], b: [f64; 3], op: Op, order: u32) -> Brush {
    let v = |p: [f64; 3]| Vec3d::new(p[0], p[1], p[2]);
    Brush::cuboid(v(a), v(b), op, order).expect("a brush")
}

fn scene_of(brushes: &[Brush]) -> Scene {
    let bsp = compile(brushes).expect("a tree");
    let faces = extract_faces(&bsp, brushes);
    let portals = Portals::build(&bsp);
    Scene::new(bsp, faces, portals, MaterialTable::default())
}

fn v(x: f32, y: f32, z: f32) -> Vec3 {
    Vec3::new(x, y, z)
}

/// A room of 20 around the origin, cut in two by a slab at x in [-1, 1].
fn split_room() -> Scene {
    scene_of(&[
        cuboid([-20.0; 3], [20.0; 3], Op::Subtract, 0),
        cuboid([-1.0, -21.0, -21.0], [1.0, 21.0, 21.0], Op::Add, 1),
    ])
}

fn lit_faces(scene: &Scene, reach: &LightReach) -> Vec<usize> {
    (0..scene.faces().len())
        .filter(|&i| reach.has_face(i))
        .collect()
}

fn white() -> Vec3 {
    v(1.0, 1.0, 1.0)
}

#[test]
fn a_wall_behind_a_wall_is_out_of_reach() {
    let scene = split_room();
    let mut light = DynLight::new();
    assert!(light.place(&scene, v(-5.0, 0.0, 0.0), 30.0, white()));
    let here = scene.bsp().leaf_at_f32(v(-5.0, 0.0, 0.0));
    let there = scene.bsp().leaf_at_f32(v(5.0, 0.0, 0.0));
    assert!(light.reaches_leaf(here));
    assert!(!light.reaches_leaf(there), "the slab fills the section");
    let reached: Vec<usize> = (0..scene.faces().len())
        .filter(|&i| light.reaches_face(i))
        .collect();
    assert!(!reached.is_empty());
    for &i in &reached {
        let face = &scene.faces()[i];
        // On the lit side of the face, and on the near side of the slab.
        assert!(face.plane.signed_distance(v(-5.0, 0.0, 0.0).to_f64()) > 0.0);
        assert!(face.front_leaves.contains(&here), "face {i}");
    }
    // The same light on the far side lights a different set of faces.
    let mut far = DynLight::new();
    assert!(far.place(&scene, v(5.0, 0.0, 0.0), 30.0, white()));
    assert!(reached.iter().all(|&i| !far.reaches_face(i)));
}

#[test]
fn only_faces_within_the_radius_and_facing_the_light_count() {
    let scene = scene_of(&[cuboid([-20.0; 3], [20.0; 3], Op::Subtract, 0)]);
    let mut reach = LightReach::default();
    scene.light_reach(v(0.0, 0.0, 0.0), 25.0, &mut reach);
    assert_eq!(lit_faces(&scene, &reach).len(), 6, "all six walls, 20 away");
    scene.light_reach(v(0.0, 0.0, 0.0), 15.0, &mut reach);
    assert!(lit_faces(&scene, &reach).is_empty(), "20 is past 15");
    scene.light_reach(v(15.0, 0.0, 0.0), 6.0, &mut reach);
    assert_eq!(
        lit_faces(&scene, &reach).len(),
        1,
        "only the wall at x = 20"
    );
}

#[test]
fn the_flood_stops_where_the_radius_does() {
    // Two rooms joined by a corridor 20 long.
    let scene = scene_of(&[
        cuboid([-30.0, -5.0, -5.0], [-10.0, 5.0, 5.0], Op::Subtract, 0),
        cuboid([10.0, -5.0, -5.0], [30.0, 5.0, 5.0], Op::Subtract, 1),
        cuboid([-10.0, -1.0, -1.0], [10.0, 1.0, 1.0], Op::Subtract, 2),
    ]);
    let second = scene.bsp().leaf_at_f32(v(20.0, 0.0, 0.0));
    let mut near = DynLight::new();
    assert!(near.place(&scene, v(-20.0, 0.0, 0.0), 15.0, white()));
    let corridor = scene.bsp().leaf_at_f32(v(-5.0, 0.0, 0.0));
    assert!(near.reaches_leaf(corridor), "the doorway is 10 away");
    assert!(!near.reaches_leaf(second), "the far room is 30 away");
    let mut wide = DynLight::new();
    assert!(wide.place(&scene, v(-20.0, 0.0, 0.0), 45.0, white()));
    assert!(wide.reaches_leaf(second));
}

#[test]
fn a_light_that_cannot_exist_reaches_nothing_and_is_dark() {
    let scene = split_room();
    let nan = f32::NAN;
    let inf = f32::INFINITY;
    let cases = [
        (v(nan, 0.0, 0.0), 10.0, white()),
        (v(0.0, inf, 0.0), 10.0, white()),
        (v(-5.0, 0.0, 0.0), nan, white()),
        (v(-5.0, 0.0, 0.0), inf, white()),
        (v(-5.0, 0.0, 0.0), 0.0, white()),
        (v(-5.0, 0.0, 0.0), -4.0, white()),
        (v(-5.0, 0.0, 0.0), 10.0, v(-1.0, 0.0, 0.0)),
        (v(-5.0, 0.0, 0.0), 10.0, v(nan, 1.0, 1.0)),
        (v(-5.0, 0.0, 0.0), 10.0, v(inf, 1.0, 1.0)),
    ];
    for (pos, radius, color) in cases {
        let mut light = DynLight::new();
        assert!(
            !light.place(&scene, pos, radius, color),
            "{pos:?} {radius} {color:?}"
        );
        assert!(!light.is_lit());
        assert!((0..scene.faces().len()).all(|i| !light.reaches_face(i)));
    }
    // Inside the slab there is no air to start the flood from.
    let mut buried = DynLight::new();
    assert!(buried.place(&scene, v(0.0, 0.0, 0.0), 10.0, white()));
    assert!((0..scene.faces().len()).all(|i| !buried.reaches_face(i)));
}

#[test]
fn big_values_are_cut_down_to_the_limits() {
    let scene = split_room();
    let mut light = DynLight::new();
    assert!(light.place(&scene, v(-5.0, 0.0, 0.0), 1.0e30, v(1.0e30, 0.5, 0.0)));
    assert_eq!(light.radius(), MAX_RADIUS);
    assert_eq!(light.color(), v(MAX_COLOR, 0.5, 0.0));
    assert!(light.set_color(v(2.0, 2.0, 2.0)));
    assert!(!light.set_color(v(f32::NAN, 0.0, 0.0)));
    assert_eq!(
        light.color(),
        v(2.0, 2.0, 2.0),
        "a refused colour changes nothing"
    );
}

#[test]
fn replacing_a_light_forgets_what_the_last_one_reached() {
    let scene = split_room();
    let mut light = DynLight::new();
    assert!(light.place(&scene, v(-5.0, 0.0, 0.0), 30.0, white()));
    let there = scene.bsp().leaf_at_f32(v(5.0, 0.0, 0.0));
    assert!(light.place(&scene, v(5.0, 0.0, 0.0), 30.0, white()));
    assert!(light.reaches_leaf(there));
    assert!(!light.reaches_leaf(scene.bsp().leaf_at_f32(v(-5.0, 0.0, 0.0))));
}
