//! Adversarial regression tests: undo during drags, lossy entity moves and
//! multi-start undo order.

use super::pointer::Pointer;
use super::*;
use mscent_math::Vec3;
use mscent_world::Entity;

fn ptr(x: f64, y: f64) -> Pointer {
    Pointer {
        view: View::Top,
        pos: [x, y],
        px: 0.1,
    }
}

fn level_with(entities: Vec<Entity>) -> Document {
    let mut doc = Document::new();
    for e in entities {
        let id = doc.alloc_id();
        doc.insert_entity_for_test(id, e);
    }
    doc
}

#[test]
fn undo_during_a_drag_does_not_leave_an_unrecorded_move() {
    let mut s = EditorState::default();
    let a = s.add_box([0.0, 0.0, 0.0], [2.0, 2.0, 2.0]).expect("a");
    s.add_box([10.0, 10.0, 0.0], [12.0, 12.0, 2.0]).expect("b");
    s.tool = Tool::Select;
    s.pointer_down(ptr(1.0, 1.0));
    s.pointer_move(ptr(5.0, 1.0));
    assert!(s.undo(), "undoes the add of b");
    s.pointer_up(ptr(5.0, 1.0));
    // The abandoned drag must not leave an edit that no undo step covers.
    assert_eq!(s.doc.brush(a).expect("a").bounds().0, [0.0, 0.0, 0.0]);
}

#[test]
fn clicking_an_entity_does_not_change_it() {
    let light = Entity::Light {
        pos: Vec3::new(1.234_567, 2.0, 3.0),
        color: Vec3::new(1.0, 1.0, 1.0),
        radius: 8.0,
        intensity: 1.0,
    };
    let mut s = EditorState::new(level_with(vec![light]));
    let before = s.doc.clone();
    s.pointer_down(ptr(1.234_567, 2.0));
    s.pointer_up(ptr(1.234_567, 2.0));
    assert_eq!(s.doc, before, "a click is not an edit");
    assert!(!s.history.can_undo());
}

#[test]
fn undoing_a_start_replacement_restores_every_entity_order() {
    let start = |x| Entity::PlayerStart {
        pos: Vec3::new(x, 0.0, 0.0),
        yaw: 0.0,
    };
    let goal = Entity::Goal {
        pos: Vec3::new(9.0, 0.0, 0.0),
        radius: 1.0,
    };
    let mut s = EditorState::new(level_with(vec![start(1.0), start(2.0), goal]));
    let before = s.doc.clone();
    s.place_entity(EntityKind::PlayerStart, [5.0, 5.0, 1.0]);
    assert_eq!(s.doc.entities().len(), 2);
    s.undo();
    assert_eq!(s.doc.entities(), before.entities());
    s.redo();
    assert_eq!(s.doc.entities().len(), 2);
}
