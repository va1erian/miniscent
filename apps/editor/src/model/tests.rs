#![allow(clippy::field_reassign_with_default)] // tests set one field after `default()` for readability

use super::document::Id;
use super::entity::{EntityKind, set_param};
use super::pointer::handles;
use super::view::Camera2D;
use super::*;
use mscent_world::{Entity, Op, showcase_level};

fn ptr(view: View, x: f64, y: f64) -> Pointer {
    Pointer {
        view,
        pos: [x, y],
        px: 0.1,
    }
}

fn drag(s: &mut EditorState, view: View, from: [f64; 2], to: [f64; 2]) {
    s.pointer_down(ptr(view, from[0], from[1]));
    s.pointer_move(ptr(view, (from[0] + to[0]) * 0.5, (from[1] + to[1]) * 0.5));
    s.pointer_up(ptr(view, to[0], to[1]));
}

/// Draws a 4x3 box at (2,2)-(6,5) with the box tool.
fn with_box() -> (EditorState, Id) {
    let mut s = EditorState::default();
    s.tool = Tool::Box;
    drag(&mut s, View::Top, [2.1, 1.9], [5.9, 5.1]);
    let Some(Selection::Brush(id)) = s.selection else {
        panic!("new brush should be selected");
    };
    s.tool = Tool::Select;
    (s, id)
}

fn bounds(s: &EditorState, id: Id) -> ([f64; 3], [f64; 3]) {
    s.doc.brush(id).expect("brush").bounds()
}

#[test]
fn box_tool_creates_a_snapped_subtract_brush() {
    let (s, id) = with_box();
    assert_eq!(bounds(&s, id), ([2.0, 2.0, 0.0], [6.0, 5.0, 4.0]));
    assert_eq!(s.doc.brush(id).unwrap().op, Op::Subtract);
}

#[test]
fn box_drawn_in_the_front_view_spans_x_and_z() {
    let mut s = EditorState::default();
    s.tool = Tool::Box;
    drag(&mut s, View::Front, [1.0, 0.0], [3.0, 2.0]);
    let id = s.doc.brushes()[0].id;
    assert_eq!(bounds(&s, id), ([1.0, 0.0, 0.0], [3.0, 4.0, 2.0]));
}

#[test]
fn zero_size_drag_creates_nothing() {
    let mut s = EditorState::default();
    s.tool = Tool::Box;
    drag(&mut s, View::Top, [3.0, 3.0], [3.2, 7.0]);
    assert!(s.doc.brushes().is_empty());
    assert!(!s.history.can_undo());
}

#[test]
fn create_undo_redo_round_trip() {
    let (mut s, id) = with_box();
    let created = s.doc.clone();
    assert!(s.undo());
    assert!(s.doc.brushes().is_empty());
    assert_eq!(s.selection, None);
    assert!(s.redo());
    assert_eq!(s.doc, created);
    assert!(s.doc.brush(id).is_some());
}

#[test]
fn move_by_dragging_snaps_and_undoes() {
    let (mut s, id) = with_box();
    let before = s.doc.clone();
    drag(&mut s, View::Top, [4.0, 3.0], [7.3, 5.0]);
    assert_eq!(bounds(&s, id), ([5.0, 4.0, 0.0], [9.0, 7.0, 4.0]));
    let moved = s.doc.clone();
    assert!(s.undo());
    assert_eq!(s.doc, before);
    assert!(s.redo());
    assert_eq!(s.doc, moved);
}

#[test]
fn a_whole_drag_is_one_undo_step() {
    let (mut s, _) = with_box();
    drag(&mut s, View::Top, [4.0, 3.0], [8.0, 6.0]);
    assert!(s.undo());
    // The next undo removes the brush itself, not part of a drag.
    assert_eq!(s.doc.brushes().len(), 1);
    assert!(s.undo());
    assert!(s.doc.brushes().is_empty());
}

#[test]
fn clicking_without_moving_records_nothing() {
    let (mut s, _) = with_box();
    s.pointer_down(ptr(View::Top, 4.0, 3.0));
    s.pointer_up(ptr(View::Top, 4.0, 3.0));
    assert!(s.undo());
    assert!(s.doc.brushes().is_empty(), "only the creation was recorded");
}

#[test]
fn resize_with_corner_and_edge_handles() {
    let (mut s, id) = with_box();
    // Top-right corner of (2,2)-(6,5) is (6,5).
    drag(&mut s, View::Top, [6.0, 5.0], [8.0, 9.0]);
    assert_eq!(bounds(&s, id), ([2.0, 2.0, 0.0], [8.0, 9.0, 4.0]));
    // Left edge midpoint is (2, 5.5).
    drag(&mut s, View::Top, [2.0, 5.5], [0.0, 5.5]);
    assert_eq!(bounds(&s, id), ([0.0, 2.0, 0.0], [8.0, 9.0, 4.0]));
    assert!(s.undo());
    assert_eq!(bounds(&s, id), ([2.0, 2.0, 0.0], [8.0, 9.0, 4.0]));
    assert!(s.undo());
    assert_eq!(bounds(&s, id), ([2.0, 2.0, 0.0], [6.0, 5.0, 4.0]));
}

#[test]
fn resize_cannot_invert_the_box() {
    let (mut s, id) = with_box();
    drag(&mut s, View::Top, [6.0, 5.0], [-10.0, -10.0]);
    let (lo, hi) = bounds(&s, id);
    assert!(hi[0] > lo[0] && hi[1] > lo[1]);
    assert_eq!(hi[0], lo[0] + 1.0);
}

#[test]
fn cancelled_drag_restores_the_brush() {
    let (mut s, id) = with_box();
    let before = s.doc.clone();
    s.pointer_down(ptr(View::Top, 4.0, 3.0));
    s.pointer_move(ptr(View::Top, 9.0, 9.0));
    assert_ne!(s.doc, before);
    s.cancel_drag();
    assert_eq!(s.doc, before);
    assert_eq!(s.selection, Some(Selection::Brush(id)));
}

#[test]
fn flip_toggles_and_undoes() {
    let (mut s, id) = with_box();
    assert!(s.flip_selected());
    assert_eq!(s.doc.brush(id).unwrap().op, Op::Add);
    assert!(s.undo());
    assert_eq!(s.doc.brush(id).unwrap().op, Op::Subtract);
    assert!(s.redo());
    assert_eq!(s.doc.brush(id).unwrap().op, Op::Add);
}

#[test]
fn delete_restores_at_the_same_index_and_id() {
    let (mut s, a) = with_box();
    s.tool = Tool::Box;
    drag(&mut s, View::Top, [10.0, 10.0], [12.0, 12.0]);
    s.tool = Tool::Select;
    let b = s.doc.brushes()[1].id;
    s.selection = Some(Selection::Brush(a));
    let before = s.doc.clone();
    assert!(s.delete_selected());
    assert_eq!(s.doc.brushes().len(), 1);
    assert_eq!(s.doc.brushes()[0].id, b);
    assert!(s.undo());
    assert_eq!(s.doc, before);
    assert_eq!(s.doc.brushes()[0].id, a);
    assert!(s.redo());
    assert_eq!(s.doc.brushes().len(), 1);
}

#[test]
fn reorder_moves_in_csg_order_and_clamps() {
    let (mut s, a) = with_box();
    s.tool = Tool::Box;
    drag(&mut s, View::Top, [10.0, 10.0], [12.0, 12.0]);
    drag(&mut s, View::Top, [20.0, 10.0], [22.0, 12.0]);
    let ids: Vec<Id> = s.doc.brushes().iter().map(|b| b.id).collect();
    s.selection = Some(Selection::Brush(a));
    assert!(s.reorder_selected(1));
    assert_eq!(s.doc.brush_index(a), Some(1));
    assert!(s.reorder_selected(5));
    assert_eq!(s.doc.brush_index(a), Some(2));
    assert!(!s.reorder_selected(1), "already last");
    assert!(s.undo());
    assert!(s.undo());
    let back: Vec<Id> = s.doc.brushes().iter().map(|b| b.id).collect();
    assert_eq!(back, ids);
    assert!(s.redo());
    assert_eq!(s.doc.brush_index(a), Some(1));
}

#[test]
fn entity_tools_place_and_move_and_undo() {
    let mut s = EditorState::default();
    s.tool = Tool::Place(EntityKind::Light);
    s.pointer_down(ptr(View::Top, 3.4, 4.6));
    let Some(Selection::Entity(id)) = s.selection else {
        panic!("placed entity should be selected");
    };
    let e = s.doc.entity(id).unwrap().entity;
    assert!(matches!(e, Entity::Light { .. }));
    assert_eq!(super::entity::pos(&e), [3.0, 5.0, 1.5]);
    s.tool = Tool::Select;
    drag(&mut s, View::Top, [3.0, 5.0], [6.0, 5.0]);
    assert_eq!(
        super::entity::pos(&s.doc.entity(id).unwrap().entity),
        [6.0, 5.0, 1.5]
    );
    assert!(s.undo());
    assert_eq!(
        super::entity::pos(&s.doc.entity(id).unwrap().entity),
        [3.0, 5.0, 1.5]
    );
    assert!(s.undo());
    assert!(s.doc.entities().is_empty());
}

#[test]
fn entity_in_the_front_view_keeps_its_hidden_depth() {
    let mut s = EditorState::default();
    s.tool = Tool::Place(EntityKind::Enemy);
    s.pointer_down(ptr(View::Front, 7.0, 2.0));
    let p = super::entity::pos(&s.doc.entities()[0].entity);
    assert_eq!(p, [7.0, 0.0, 2.0]);
}

#[test]
fn second_player_start_replaces_the_first_in_one_undo_step() {
    let mut s = EditorState::default();
    s.tool = Tool::Place(EntityKind::PlayerStart);
    s.pointer_down(ptr(View::Top, 1.0, 1.0));
    let first = s.doc.entities()[0].id;
    s.pointer_down(ptr(View::Top, 9.0, 9.0));
    assert_eq!(s.doc.entities().len(), 1);
    assert_ne!(s.doc.entities()[0].id, first);
    assert!(s.undo());
    assert_eq!(s.doc.entities()[0].id, first);
}

#[test]
fn pick_prefers_entities_then_the_smallest_brush() {
    let (mut s, small) = with_box();
    s.tool = Tool::Box;
    drag(&mut s, View::Top, [0.0, 0.0], [20.0, 20.0]);
    s.tool = Tool::Select;
    assert_eq!(
        s.pick(ptr(View::Top, 3.0, 3.0)),
        Some(Selection::Brush(small))
    );
    s.tool = Tool::Place(EntityKind::Goal);
    s.pointer_down(ptr(View::Top, 3.0, 3.0));
    let goal = s.doc.entities()[0].id;
    assert_eq!(
        s.pick(ptr(View::Top, 3.05, 3.0)),
        Some(Selection::Entity(goal))
    );
    assert_eq!(s.pick(ptr(View::Top, 50.0, 50.0)), None);
}

#[test]
fn property_edits_validate_and_undo() {
    let (mut s, id) = with_box();
    assert!(s.edit_selected_brush(|b| b.material = 3));
    assert_eq!(s.doc.brush(id).unwrap().material, 3);
    let bad = s.edit_selected_brush(|b| {
        if let Shape::Box { min, .. } = &mut b.shape {
            min[0] = 100.0;
        }
    });
    assert!(!bad, "inverted box is rejected");
    assert!(s.undo());
    assert_eq!(s.doc.brush(id).unwrap().material, 0);
}

#[test]
fn entity_property_edit_rejects_invalid_values() {
    let mut s = EditorState::default();
    s.tool = Tool::Place(EntityKind::Light);
    s.pointer_down(ptr(View::Top, 0.0, 0.0));
    assert!(s.edit_selected_entity(|e| {
        if let Entity::Light { radius, .. } = e {
            *radius = 30.0;
        }
    }));
    assert!(!s.edit_selected_entity(|e| {
        if let Entity::Light { radius, .. } = e {
            *radius = -1.0;
        }
    }));
    assert!(s.undo());
    let Entity::Light { radius, .. } = s.doc.entities()[0].entity else {
        unreachable!()
    };
    assert_eq!(radius, 16.0);
}

#[test]
fn new_action_after_undo_clears_redo() {
    let (mut s, _) = with_box();
    assert!(s.undo());
    assert!(s.history.can_redo());
    s.tool = Tool::Box;
    drag(&mut s, View::Top, [0.0, 0.0], [2.0, 2.0]);
    assert!(!s.history.can_redo());
    assert!(!s.redo());
}

#[test]
fn a_long_session_unwinds_to_the_empty_document_and_back() {
    let (mut s, a) = with_box();
    s.flip_selected();
    s.edit_selected_brush(|b| b.material = 2);
    s.tool = Tool::Place(EntityKind::Enemy);
    s.pointer_down(ptr(View::Top, 3.0, 3.0));
    s.tool = Tool::Select;
    s.selection = Some(Selection::Brush(a));
    drag(&mut s, View::Top, [4.0, 3.5], [6.0, 3.5]);
    s.delete_selected();
    let end = s.doc.clone();
    let mut steps = 0;
    while s.undo() {
        steps += 1;
    }
    assert!(steps >= 6);
    assert!(s.doc.brushes().is_empty() && s.doc.entities().is_empty());
    while s.redo() {}
    assert_eq!(s.doc, end);
}

#[test]
fn showcase_imports_boxes_and_exports_a_compilable_level() {
    let level = showcase_level();
    let doc = Document::from_level(&level);
    assert_eq!(doc.brushes().len(), level.brushes.len());
    assert_eq!(doc.entities().len(), level.entities.len());
    let boxes = doc.brushes().iter().filter(|b| b.is_box()).count();
    assert_eq!(boxes, doc.brushes().len() - 1, "only the ramp is not a box");
    let out = doc.to_level().expect("exports");
    assert_eq!(out.brushes, level.brushes, "round trip keeps every brush");
    assert_eq!(out.entities, level.entities);
    assert!(mscent_world::compile(&out.brushes).is_ok());
}

#[test]
fn the_ramp_moves_but_does_not_resize() {
    let mut s = EditorState::new(Document::from_level(&showcase_level()));
    let ramp = s.doc.brushes().iter().find(|b| !b.is_box()).unwrap().id;
    s.selection = Some(Selection::Brush(ramp));
    let (lo, hi) = bounds(&s, ramp);
    s.pointer_down(ptr(View::Top, lo[0] + 0.5, lo[1] + 0.5));
    s.pointer_move(ptr(View::Top, lo[0] + 2.5, lo[1] + 0.5));
    s.pointer_up(ptr(View::Top, lo[0] + 2.5, lo[1] + 0.5));
    let (lo2, hi2) = bounds(&s, ramp);
    assert!((lo2[0] - lo[0] - 2.0).abs() < 1e-3 && (hi2[0] - hi[0] - 2.0).abs() < 1e-3);
    assert!(s.doc.to_level().is_ok());
}

#[test]
fn camera_round_trips_and_zooms_about_the_cursor() {
    let size = (400.0, 300.0);
    let mut c = Camera2D {
        center: [10.0, 5.0],
        scale: 8.0,
    };
    let (x, y) = c.to_screen([12.0, 7.0], size);
    let w = c.to_world(x, y, size);
    assert!((w[0] - 12.0).abs() < 1e-9 && (w[1] - 7.0).abs() < 1e-9);
    let under = c.to_world(100.0, 60.0, size);
    c.zoom_at(100.0, 60.0, size, 2.0);
    let after = c.to_world(100.0, 60.0, size);
    assert!((under[0] - after[0]).abs() < 1e-9 && (under[1] - after[1]).abs() < 1e-9);
    assert!((c.scale - 16.0).abs() < 1e-9);
    let x0 = c.center[0];
    c.pan_pixels(16.0, 0.0);
    assert!(
        (x0 - c.center[0] - 1.0).abs() < 1e-9,
        "dragging right moves the view left"
    );
}

#[test]
fn eight_handles_surround_a_rectangle() {
    let h = handles([0.0, 0.0], [4.0, 2.0]);
    assert_eq!(h.len(), 8);
    assert!(
        h.iter()
            .any(|(k, p)| k.dx == 1 && k.dy == 1 && *p == [4.0, 2.0])
    );
    assert!(
        h.iter()
            .any(|(k, p)| k.dx == 0 && k.dy == -1 && *p == [2.0, 0.0])
    );
}

#[test]
fn world_entity_edits_fog_with_undo_and_there_is_only_one() {
    let mut s = EditorState::default();
    s.place_entity(EntityKind::World, [0.0, 0.0, 1.0]);
    let second = s.place_entity(EntityKind::World, [2.0, 0.0, 1.0]);
    let worlds = |s: &EditorState| {
        s.doc
            .entities()
            .iter()
            .filter(|e| matches!(e.entity, Entity::World { .. }))
            .count()
    };
    assert_eq!(worlds(&s), 1, "a second world replaces the first");
    assert_eq!(s.selection, Some(Selection::Entity(second)));
    assert!(s.edit_selected_entity(|e| set_param(e, 1, 90.0)));
    assert_eq!(s.doc.to_level().unwrap().fog(), Some((4.0, 90.0)));
    assert!(
        !s.edit_selected_entity(|e| set_param(e, 0, 200.0)),
        "start past end"
    );
    assert!(s.undo());
    assert_eq!(s.doc.to_level().unwrap().fog(), Some((4.0, 26.0)));
}

#[test]
fn volumes_survive_loading_into_a_document_and_saving_again() {
    use mscent_world::voxel::{CellSize, Volume};
    let mut level = mscent_world::showcase_level();
    let last = level
        .brushes
        .iter()
        .map(|b| b.order())
        .max()
        .expect("brushes");
    let volume = Volume::new(Op::Add, last + 5, CellSize::Quarter, [0; 3], [1, 1, 1]);
    level.volumes.push(volume.expect("valid"));
    let doc = Document::from_level(&level);
    let saved = doc.to_level().expect("exports");
    let renumbered = level.volumes[0]
        .clone()
        .with_order(level.brushes.len() as u32);
    assert_eq!(saved.volumes, [renumbered]);
    let built = crate::build::validate(&doc).expect("valid");
    assert_eq!(built.volumes, saved.volumes);
}

/// The volume's place among the brushes, as the orders of the brushes around it.
fn sides_of_volume(doc: &Document) -> (Vec<usize>, Vec<usize>) {
    let level = doc.to_level().expect("exports");
    let at = level.volumes[0].order();
    let ids = |keep: &dyn Fn(u32) -> bool| {
        (level.brushes.iter().enumerate())
            .filter(|(_, b)| keep(b.order()))
            .map(|(i, _)| doc.brushes()[i].id as usize)
            .collect()
    };
    (ids(&|o| o < at), ids(&|o| o > at))
}

#[test]
fn a_volume_keeps_its_place_among_brushes_across_saving_and_edits() {
    use crate::model::document::BrushDef;
    use mscent_math::Vec3d;
    use mscent_world::Level;
    use mscent_world::voxel::{CellSize, Volume};
    let cube = |x: f64, order: u32| {
        let lo = Vec3d::new(x, 0.0, 0.0);
        mscent_world::Brush::cuboid(lo, Vec3d::new(x + 1.0, 1.0, 1.0), Op::Add, order)
            .expect("valid")
    };
    let level = Level {
        brushes: vec![cube(0.0, 10), cube(2.0, 20), cube(4.0, 30)],
        entities: Vec::new(),
        volumes: vec![Volume::new(Op::Add, 25, CellSize::Quarter, [0; 3], [1, 1, 1]).expect("ok")],
    };
    let mut doc = Document::from_level(&level);
    let ids: Vec<usize> = doc.brushes().iter().map(|b| b.id as usize).collect();
    let (a, b, c) = (ids[0], ids[1], ids[2]);
    assert_eq!(sides_of_volume(&doc), (vec![a, b], vec![c]));

    // Saving and loading again keeps it, and every order stays distinct.
    let again = Document::from_level(&doc.to_level().expect("exports"));
    assert_eq!(sides_of_volume(&again).0.len(), 2);

    // Deleting the brush right before it leaves it after the one before that,
    // and undoing the delete restores the place exactly.
    let (at, removed) = doc.remove_brush(b as u32).expect("there");
    assert_eq!(sides_of_volume(&doc), (vec![a], vec![c]));
    doc.insert_brush(at, removed);
    assert_eq!(sides_of_volume(&doc), (vec![a, b], vec![c]));

    // A brush added at the end comes after the volume and cuts it.
    let added = doc.alloc_id();
    let def = BrushDef::from_brush(added, &cube(6.0, 0));
    doc.insert_brush(3, def);
    assert_eq!(sides_of_volume(&doc), (vec![a, b], vec![c, added as usize]));
}
