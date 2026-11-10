//! Resizing a voxel volume by number and by handle.

use mscent_world::voxel::{CellSize, Volume};
use mscent_world::{Brush, Level, Op};

use super::document::{Document, Id};
use super::pointer::Pointer;
use super::state::{EditorState, Selection};
use super::view::View;

/// A room with a 2x2x1 chunk subtract volume (8 by 8 by 4 units) from the
/// origin, selected.
fn with_volume() -> (EditorState, Id) {
    let room = Brush::cuboid(
        mscent_math::Vec3d::new(-4.0, -4.0, -4.0),
        mscent_math::Vec3d::new(20.0, 20.0, 12.0),
        Op::Subtract,
        0,
    )
    .expect("valid");
    let volume = Volume::new(Op::Subtract, 1, CellSize::Quarter, [0; 3], [2, 2, 1]).expect("valid");
    let level = Level {
        brushes: vec![room],
        volumes: vec![volume],
        ..Default::default()
    };
    let mut st = EditorState {
        doc: Document::from_level(&level),
        ..Default::default()
    };
    let id = st.doc.volumes().next().expect("a volume").0.id;
    st.selection = Some(Selection::Brush(id));
    (st, id)
}

fn bounds(st: &EditorState, id: Id) -> ([f64; 3], [f64; 3]) {
    st.doc.brush(id).expect("brush").bounds()
}

fn chunks(st: &EditorState, id: Id) -> [u16; 3] {
    st.doc
        .brush(id)
        .and_then(|b| b.volume())
        .expect("volume")
        .chunks()
}

#[test]
fn a_volume_edge_typed_in_lands_on_a_chunk_edge() {
    let (mut st, id) = with_volume();
    assert!(st.set_selected_edge(0, true, 13.0));
    assert_eq!(bounds(&st, id).1[0], 12.0, "13 is nearest the edge at 12");
    assert_eq!(chunks(&st, id), [3, 2, 1]);
    assert!(st.set_selected_edge(2, false, -3.9));
    assert_eq!(bounds(&st, id).0[2], -4.0);
    assert_eq!(chunks(&st, id), [3, 2, 2]);
    // One undo step each.
    assert!(st.undo());
    assert_eq!(chunks(&st, id), [3, 2, 1]);
    assert!(st.undo());
    assert_eq!(chunks(&st, id), [2, 2, 1]);
}

#[test]
fn an_edge_that_rounds_to_where_it_is_changes_nothing_and_says_so() {
    let (mut st, id) = with_volume();
    assert!(!st.set_selected_edge(0, true, 8.5));
    assert_eq!(bounds(&st, id).1[0], 8.0);
    assert!(st.message.contains("chunks of 4"), "{}", st.message);
}

#[test]
fn a_volume_cannot_lose_its_last_chunk_or_overlap_another() {
    let (mut st, id) = with_volume();
    assert!(!st.set_selected_edge(0, true, 0.0), "no width left");
    assert!(!st.set_selected_edge(0, true, f64::NAN));
    assert!(!st.set_selected_edge(0, true, 1.0e9), "too many chunks");
    assert_eq!(bounds(&st, id), ([0.0; 3], [8.0, 8.0, 4.0]));
    // A second volume next door: growing into it is refused.
    let other =
        Volume::new(Op::Subtract, 2, CellSize::Quarter, [48, 0, 0], [1, 1, 1]).expect("valid");
    let mut level = st.doc.to_level().expect("exports");
    level.volumes.push(other);
    st.doc = Document::from_level(&level);
    let id = st.doc.volumes().next().expect("a volume").0.id;
    st.selection = Some(Selection::Brush(id));
    assert!(!st.set_selected_edge(0, true, 16.0));
    assert!(st.message.contains("overlap"), "{}", st.message);
    assert_eq!(chunks(&st, id), [2, 2, 1]);
}

#[test]
fn dragging_a_volumes_handle_resizes_it_by_chunks() {
    let (mut st, id) = with_volume();
    let at = |x: f64, y: f64| Pointer {
        view: View::Top,
        pos: [x, y],
        px: 0.1,
    };
    // The top right corner is at (8, 8).
    st.pointer_down(at(8.0, 8.0));
    st.pointer_move(at(10.0, 10.0));
    st.pointer_up(at(12.0, 12.0));
    assert_eq!(chunks(&st, id), [3, 3, 1]);
    assert_eq!(bounds(&st, id).1, [12.0, 12.0, 4.0]);
    assert!(st.undo());
    assert_eq!(chunks(&st, id), [2, 2, 1]);
}
