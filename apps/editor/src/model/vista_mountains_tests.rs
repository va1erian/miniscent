//! New Mountains: a miniature room, one undo step, the next layer of the vista.

use mscent_world::materials::{VIEW_FIRST, view_material};
use mscent_world::{Entity, Op, vista_warnings};

use super::document::Shape;
use super::entity::EntityKind;
use super::state::EditorState;

/// A play room, 16 by 16 by 8 with a player start.
fn level_with_room() -> EditorState {
    let mut st = EditorState::default();
    st.add_box([0.0; 3], [16.0, 16.0, 8.0]).expect("room");
    st.place_entity(EntityKind::PlayerStart, [8.0, 8.0, 2.0]);
    st
}

fn layers(st: &EditorState) -> Vec<Entity> {
    let all = st.doc.entities().iter().map(|e| e.entity);
    all.filter(|e| matches!(e, Entity::VistaLayer { .. }))
        .collect()
}

#[test]
fn new_mountains_builds_the_room_in_one_undo_step_and_undoes() {
    let mut st = level_with_room();
    let before = (st.doc.brushes().len(), st.doc.entities().len());
    let depth = st.history.depth();
    assert!(st.new_mountains(), "{}", st.message);
    assert_eq!(st.history.depth(), depth + 1);
    assert_eq!(st.doc.brushes().len(), before.0 + 3);
    assert_eq!(st.doc.entities().len(), before.1 + 2);
    let shapes: Vec<bool> = (st.doc.brushes()[before.0..].iter())
        .map(|b| matches!(b.shape, Shape::Volume(_)))
        .collect();
    assert_eq!(shapes, [false, false, true]);
    let room = &st.doc.brushes()[before.0];
    assert_eq!(
        (room.op, room.material),
        (Op::Subtract, view_material(0).unwrap())
    );
    assert!(room.material >= VIEW_FIRST);
    let [
        Entity::VistaLayer {
            slot,
            layer,
            parallax,
            ..
        },
    ] = layers(&st)[..]
    else {
        panic!("one layer");
    };
    assert_eq!((slot, layer, parallax), (0, 0, 0.0625));
    // It exports as a level that loads, and raises no warning but the gradient
    // behind its walls (no layer behind it).
    let level = st.doc.to_level().expect("exports");
    assert_eq!(level.volumes.len(), 1);
    assert!(
        vista_warnings(&level).is_empty(),
        "{:?}",
        vista_warnings(&level)
    );
    assert!(st.undo());
    assert_eq!((st.doc.brushes().len(), st.doc.entities().len()), before);
}

#[test]
fn it_takes_the_next_layer_of_the_existing_vista_and_stops_when_they_are_full() {
    let mut st = level_with_room();
    assert!(st.new_vista());
    let centre = st.vista_centre();
    for expected in 1..=3u8 {
        assert!(st.new_mountains(), "{}", st.message);
        let Some(Entity::VistaLayer {
            slot,
            layer,
            centre: c,
            ..
        }) = layers(&st).last().copied()
        else {
            panic!("a layer");
        };
        assert_eq!((slot, layer, c), (0, expected, centre));
    }
    let count = (st.doc.brushes().len(), st.doc.entities().len());
    assert!(!st.new_mountains());
    assert!(st.message.contains("all 4 layers"), "{}", st.message);
    assert_eq!((st.doc.brushes().len(), st.doc.entities().len()), count);
}

#[test]
fn the_rooms_floor_continues_the_play_floor() {
    let mut st = level_with_room();
    assert!(st.new_mountains());
    let Some(Entity::VistaLayer {
        pos,
        centre,
        parallax,
        ..
    }) = layers(&st).last().copied()
    else {
        panic!("a layer");
    };
    let floor = st.doc.brushes()[st.doc.brushes().len() - 2].bounds().1[2];
    // The play floor (z 0) maps to the top of the room's floor.
    let at = f64::from(pos.z) + (0.0 - f64::from(centre.z)) * f64::from(parallax);
    assert!((at - floor).abs() < 1.0e-3, "{at} against {floor}");
}
