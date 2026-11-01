//! A level's names and mover groups must survive the editor: they are keyed by
//! id, so reordering and deleting cannot point them at the wrong thing.

use super::document::Document;
use mscent_math::{Vec3, Vec3d};
use mscent_world::{Brush, Entity, EntityLabel, Level, MoverGroup, Op, format};

fn cuboid(x: f64, op: Op, order: u32) -> Brush {
    let (lo, hi) = (Vec3d::new(x, 0.0, 0.0), Vec3d::new(x + 1.0, 1.0, 1.0));
    Brush::cuboid(lo, hi, op, order).unwrap()
}

fn marker(x: f32) -> Entity {
    let pos = Vec3::new(x, 0.0, 0.0);
    Entity::Marker { pos, yaw: 0.0 }
}

/// Stored out of CSG order, so list position and file index differ.
fn level() -> Level {
    Level {
        brushes: vec![
            cuboid(4.0, Op::Add, 2),
            cuboid(0.0, Op::Subtract, 0),
            cuboid(2.0, Op::Add, 1),
        ],
        entities: vec![marker(1.0), marker(2.0)],
        labels: vec![EntityLabel {
            entity: 1,
            name: Some("hinge".to_string()),
            tags: vec!["door_parts".to_string()],
        }],
        mover_groups: vec![MoverGroup {
            name: "door".to_string(),
            tags: Vec::new(),
            brushes: vec![0],
        }],
    }
}

fn moving_order(l: &Level) -> u32 {
    l.brushes[l.mover_groups[0].brushes[0] as usize].order()
}

#[test]
fn labels_survive_load_and_export_on_the_right_things() {
    let doc = Document::from_level(&level());
    let out = doc.to_level().unwrap();
    assert_eq!(out.labels, level().labels);
    assert_eq!(out.check_labels(), Ok(()));
    // The group followed its brush from file index 0 to list position 2, and
    // only the moving brush left the CSG.
    assert_eq!(out.mover_groups[0].brushes, [2]);
    assert_eq!(moving_order(&out), 2);
    assert_eq!(out.static_brushes().len(), 2);
}

#[test]
fn labels_survive_a_save_and_reload() {
    let out = Document::from_level(&level()).to_level().unwrap();
    let bytes = format::write(&out);
    let back = Document::from_level(&format::read(&bytes).unwrap());
    assert_eq!(back.to_level().unwrap(), out);
}

#[test]
fn deleting_an_earlier_thing_keeps_labels_on_the_right_ones() {
    let mut doc = Document::from_level(&level());
    let first_entity = doc.entities()[0].id;
    let first_brush = doc.brushes()[0].id;
    doc.remove_entity(first_entity);
    doc.remove_brush(first_brush);
    let out = doc.to_level().unwrap();
    assert_eq!(out.labels[0].entity, 0);
    assert_eq!(out.labels[0].name.as_deref(), Some("hinge"));
    assert_eq!(out.mover_groups[0].brushes, [1]);
    assert_eq!(out.check_labels(), Ok(()));
}

#[test]
fn deleting_a_group_or_label_owner_drops_it_and_undo_brings_it_back() {
    let mut doc = Document::from_level(&level());
    let labelled = doc.entities()[1].id;
    let moving = doc.groups()[0].brushes[0];
    let (ei, e) = doc.remove_entity(labelled).unwrap();
    let (bi, b) = doc.remove_brush(moving).unwrap();
    let out = doc.to_level().unwrap();
    assert!(out.labels.is_empty() && out.mover_groups.is_empty());
    assert_eq!(out.check_labels(), Ok(()));
    doc.insert_entity(ei, e);
    doc.insert_brush(bi, b);
    let out = doc.to_level().unwrap();
    assert_eq!(out.labels.len(), 1);
    assert_eq!(out.mover_groups.len(), 1);
}

#[test]
fn a_mover_made_subtractive_leaves_its_group_instead_of_breaking_the_level() {
    let mut doc = Document::from_level(&level());
    let moving = doc.groups()[0].brushes[0];
    let mut def = doc.brush(moving).unwrap().clone();
    def.op = Op::Subtract;
    doc.replace_brush(&def);
    let out = doc.to_level().unwrap();
    assert!(out.mover_groups.is_empty());
    assert_eq!(out.check_labels(), Ok(()));
}

#[test]
fn a_change_of_mover_group_counts_as_a_change_of_geometry() {
    let doc = Document::from_level(&level());
    let mut plain = level();
    plain.mover_groups.clear();
    assert!(doc.same_geometry(&doc.clone()));
    assert!(!doc.same_geometry(&Document::from_level(&plain)));
}
