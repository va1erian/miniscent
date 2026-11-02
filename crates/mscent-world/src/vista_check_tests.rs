use super::*;
use crate::brush::{Brush, Op};
use crate::labels::MoverGroup;
use crate::materials::{VIEW_FIRST, view_material};
use mscent_math::{Vec3, Vec3d};

fn cuboid(lo: [f64; 3], hi: [f64; 3], op: Op, order: u32, material: u16) -> Brush {
    let v = |a: [f64; 3]| Vec3d::new(a[0], a[1], a[2]);
    Brush::cuboid(v(lo), v(hi), op, order)
        .unwrap()
        .with_material(material)
}

/// A play room at the origin, 32 wide, with its walls on `view`, and a sealed
/// room of 64 far away at x = 1000.
fn level(view: u16) -> Level {
    Level {
        brushes: vec![
            cuboid([0.0; 3], [32.0; 3], Op::Subtract, 0, view),
            cuboid(
                [1000.0, 0.0, 0.0],
                [1064.0, 64.0, 64.0],
                Op::Subtract,
                1,
                10,
            ),
        ],
        entities: vec![Entity::PlayerStart {
            pos: Vec3::new(16.0, 16.0, 10.0),
            yaw: 0.0,
        }],
        ..Level::default()
    }
}

fn layer_at(pos: Vec3, slot: u8, layer: u8) -> Entity {
    Entity::VistaLayer {
        pos,
        centre: Vec3::new(16.0, 16.0, 16.0),
        slot,
        layer,
        parallax: 0.0,
        spin: Vec3::new(0.0, 0.0, 0.0),
        pan: [0.0, 0.0],
        fog_start: 0.0,
        fog_end: crate::MAX_VISTA_FOG,
        fog_color: 0,
    }
}

fn far_layer(slot: u8, layer: u8) -> Entity {
    layer_at(Vec3::new(1032.0, 32.0, 32.0), slot, layer)
}

#[test]
fn a_level_without_vistas_has_nothing_to_say() {
    assert!(vista_warnings(&level(0)).is_empty());
    assert!(vista_warnings(&Level::default()).is_empty());
}

#[test]
fn a_sealed_layer_fed_to_a_view_wall_is_clean() {
    let mut l = level(VIEW_FIRST);
    l.entities.push(far_layer(0, 0));
    assert_eq!(vista_warnings(&l), []);
}

#[test]
fn a_layer_in_the_players_room_is_a_leak() {
    let mut l = level(VIEW_FIRST);
    l.entities.push(layer_at(Vec3::new(8.0, 8.0, 8.0), 0, 1));
    assert_eq!(
        vista_warnings(&l),
        [VistaWarning::SharesPlayerIsland { slot: 0, layer: 1 }]
    );
}

#[test]
fn a_layer_in_solid_shows_nothing() {
    let mut l = level(VIEW_FIRST);
    l.entities
        .push(layer_at(Vec3::new(500.0, 500.0, 500.0), 0, 0));
    assert_eq!(
        vista_warnings(&l),
        [VistaWarning::LayerInSolid { slot: 0, layer: 0 }]
    );
}

#[test]
fn a_view_wall_without_a_layer_shows_the_gradient() {
    let view_two = view_material(2).unwrap();
    let mut l = level(view_two);
    l.entities.push(far_layer(0, 0));
    assert_eq!(
        vista_warnings(&l),
        [VistaWarning::SlotWithoutLayer { slot: 2 }]
    );
    l.entities.clear();
    assert_eq!(
        vista_warnings(&l),
        [VistaWarning::SlotWithoutLayer { slot: 2 }]
    );
}

#[test]
fn a_buried_view_plane_is_not_a_face_that_shows() {
    // The add brush covers the view room's floor plane's whole face.
    let mut l = level(VIEW_FIRST);
    l.brushes.push(cuboid(
        [-10.0; 3],
        [42.0; 3],
        Op::Add,
        2,
        crate::brush::DEFAULT_MATERIAL,
    ));
    assert_eq!(vista_warnings(&l), []);
}

#[test]
fn a_mover_with_a_view_face_is_named() {
    let mut l = level(0);
    l.brushes
        .push(cuboid([4.0; 3], [8.0; 3], Op::Add, 2, VIEW_FIRST));
    l.mover_groups.push(MoverGroup {
        name: "lift".into(),
        tags: Vec::new(),
        brushes: vec![2],
    });
    assert_eq!(
        vista_warnings(&l),
        [VistaWarning::MoverWithViewport {
            group: "lift".into()
        }]
    );
}

#[test]
fn a_level_with_no_player_start_has_no_leak() {
    let mut l = level(VIEW_FIRST);
    l.entities.clear();
    l.entities.push(layer_at(Vec3::new(8.0, 8.0, 8.0), 0, 0));
    assert_eq!(vista_warnings(&l), []);
}

#[test]
fn junk_in_the_level_never_panics() {
    let mut l = level(VIEW_FIRST);
    l.entities
        .push(layer_at(Vec3::new(f32::NAN, 0.0, 0.0), 0, 0));
    l.entities.push(layer_at(Vec3::new(0.0, 0.0, 0.0), 200, 9));
    l.mover_groups.push(MoverGroup {
        name: "x".into(),
        tags: Vec::new(),
        brushes: vec![u32::MAX],
    });
    let _ = vista_warnings(&l);
    for w in vista_warnings(&l) {
        assert!(!w.to_string().is_empty());
    }
}
