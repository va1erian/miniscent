//! Every basic entity, placed in a real room, run by the real game.

use super::*;
use crate::model::{Document, EntityKind};
use crate::script::check::check;
use mscent_world::Level;

/// One room, 32 by 12 by 8, with the player in it.
fn room() -> EditorState {
    let mut st = EditorState::new(Document::new());
    st.new_op = Op::Subtract;
    st.add_box([-16.0, -6.0, -4.0], [16.0, 6.0, 4.0]).unwrap();
    st.place_entity(EntityKind::PlayerStart, [-12.0, 0.0, 0.0]);
    st.history.clear();
    st
}

fn level(st: &EditorState) -> Level {
    st.doc.to_level().expect("a valid level")
}

fn names(st: &EditorState) -> Vec<String> {
    let level = level(st);
    let labels = level.labels.iter().filter_map(|l| l.name.clone());
    let groups = level.mover_groups.iter().map(|g| g.name.clone());
    groups.chain(labels).collect()
}

fn run_script_clean(st: &EditorState) {
    let report = check(level(st), None, st.script.text());
    assert_eq!(report.problems, [], "script:\n{}", st.script.text());
}

#[test]
fn every_entity_makes_a_valid_level_whose_script_runs_in_the_game() {
    for entry in &KIT {
        let mut st = room();
        assert!(st.place_kit(entry.kind, [4.0, 0.0, 1.0]), "{}", entry.label);
        level(&st).check_labels().expect("names that save");
        assert!(!st.script.text().is_empty(), "{}", entry.label);
        run_script_clean(&st);
    }
}

#[test]
fn every_entity_together_in_one_level_keeps_its_names_apart() {
    let mut st = room();
    for entry in KIT.iter().chain(&KIT) {
        assert!(st.place_kit(entry.kind, [4.0, 0.0, 1.0]), "{}", entry.label);
    }
    let names = names(&st);
    let mut unique = names.clone();
    unique.sort();
    unique.dedup();
    assert_eq!(names.len(), unique.len(), "{names:?}");
    assert!(names.contains(&"door_2".to_string()), "{names:?}");
    level(&st).check_labels().expect("names that save");
    run_script_clean(&st);
}

#[test]
fn undo_takes_a_whole_entity_out_in_one_step_and_the_check_says_what_is_left() {
    let mut st = room();
    let before = st.doc.clone();
    assert!(st.place_kit(KitKind::LockedDoor, [4.0, 0.0, 1.0]));
    assert_eq!(st.history.depth(), 1);
    assert!(st.undo());
    assert_eq!(st.doc.brushes(), before.brushes());
    assert_eq!(st.doc.entities(), before.entities());
    assert_eq!(st.doc.groups(), before.groups());
    assert!(names(&st).is_empty());
    let report = check(level(&st), None, st.script.text());
    assert!(!report.problems.is_empty(), "the script names what is gone");
    assert!(st.redo());
    run_script_clean(&st);
}

#[test]
fn a_door_closes_the_room_off_and_clears_it_when_open() {
    let mut st = room();
    assert!(st.place_kit(KitKind::Door, [4.0, 0.0, 1.0]));
    let level = level(&st);
    let (lo, hi) = st
        .doc
        .brushes()
        .iter()
        .find(|b| b.op == Op::Add)
        .unwrap()
        .bounds();
    assert_eq!((lo[0], hi[0]), (3.5, 4.5), "one thick, at the click");
    assert_eq!((lo[1], hi[1]), (-6.0, 6.0), "wall to wall");
    assert_eq!((lo[2], hi[2]), (-4.0, 4.0), "floor to ceiling");
    assert_eq!(level.mover_groups[0].name, "door_1");
    assert!(st.script.text().contains("z: 9"), "{}", st.script.text());
}

#[test]
fn a_door_in_a_room_running_the_other_way_turns_with_it() {
    let mut st = EditorState::new(Document::new());
    st.new_op = Op::Subtract;
    st.add_box([-6.0, -16.0, -4.0], [6.0, 16.0, 4.0]).unwrap();
    assert!(st.place_kit(KitKind::Door, [0.0, 3.0, 1.0]));
    let (lo, hi) = st
        .doc
        .brushes()
        .iter()
        .find(|b| b.op == Op::Add)
        .unwrap()
        .bounds();
    assert_eq!((lo[1], hi[1]), (2.5, 3.5));
    assert_eq!((lo[0], hi[0]), (-6.0, 6.0));
}

#[test]
fn outside_any_room_a_door_still_has_a_size() {
    let mut st = EditorState::new(Document::new());
    assert!(st.place_kit(KitKind::Door, [50.0, 50.0, 0.0]));
    let brush = st.doc.brushes().iter().find(|b| b.op == Op::Add).unwrap();
    assert!(crate::model::state::box_ok(
        brush.bounds().0,
        brush.bounds().1
    ));
}

#[test]
fn off_the_map_nothing_is_placed_or_written() {
    let mut st = room();
    let before = st.doc.clone();
    assert!(!st.place_kit(KitKind::Door, [LIMIT, 0.0, 0.0]));
    assert_eq!(st.doc, before);
    assert!(st.script.text().is_empty());
    assert!(st.message.contains("Cannot"), "{}", st.message);
}

#[test]
fn clicking_with_a_picked_entity_places_it_and_goes_back_to_select() {
    use crate::model::{Pointer, View};
    let mut st = room();
    st.arm_kit(KitKind::Exit);
    assert_eq!(st.tool, Tool::Kit(KitKind::Exit));
    assert!(st.message.contains("Click"), "{}", st.message);
    st.pointer_down(Pointer {
        view: View::ALL[0],
        pos: [3.0, 2.0],
        px: 0.1,
    });
    assert_eq!(st.tool, Tool::Select);
    assert!(names(&st).contains(&"exit_1".to_string()));
    assert!(st.script.text().contains("exitZone: 'exit_1'"));
}
