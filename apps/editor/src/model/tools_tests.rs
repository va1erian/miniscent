//! The slice, carve, vertex and extrude tools, driven through the same
//! pointer API the views use and checked against the geometry they leave.

use mscent_world::Op;

use super::document::{BrushDef, Id, Shape};
use super::pick::FaceRef;
use super::*;

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

/// Draws a box with the box tool in the top view (z spans 0..4) and
/// selects it.
fn draw(s: &mut EditorState, lo: [f64; 2], hi: [f64; 2]) -> Id {
    s.set_tool(Tool::Box);
    drag(s, View::Top, lo, hi);
    let Some(Selection::Brush(id)) = s.selection else {
        panic!("new brush should be selected");
    };
    id
}

fn one_box() -> (EditorState, Id) {
    let mut s = EditorState::default();
    let id = draw(&mut s, [0.0, 0.0], [4.0, 4.0]);
    (s, id)
}

fn bounds(s: &EditorState, id: Id) -> ([f64; 3], [f64; 3]) {
    s.doc.brush(id).expect("brush").bounds()
}

fn volume(b: &BrushDef) -> f64 {
    let Shape::Box { min, max } = &b.shape else {
        panic!("expected a box");
    };
    (0..3).map(|i| max[i] - min[i]).product()
}

#[test]
fn slice_keeping_both_sides_makes_two_boxes() {
    let (mut s, id) = one_box();
    s.set_tool(Tool::Slice);
    // Drawn along +y at x = 1: its right is +x, the big piece.
    drag(&mut s, View::Top, [1.0, -1.0], [1.0, 5.0]);
    assert!(s.cut.is_some(), "the line waits for Enter");
    assert!(s.apply_slice());
    assert_eq!(s.doc.brushes().len(), 2);
    assert_eq!(bounds(&s, id), ([0.0, 0.0, 0.0], [1.0, 4.0, 4.0]));
    let other = s.doc.brushes()[1].clone();
    assert_eq!(other.bounds(), ([1.0, 0.0, 0.0], [4.0, 4.0, 4.0]));
    assert_eq!(other.op, s.doc.brushes()[0].op);
    assert!(s.cut.is_none());
    assert!(s.undo());
    assert_eq!(s.doc.brushes().len(), 1);
    assert_eq!(bounds(&s, id), ([0.0, 0.0, 0.0], [4.0, 4.0, 4.0]));
}

#[test]
fn slice_keeping_one_side_trims_the_brush_and_keeps_its_id() {
    for (keep, want) in [
        (Keep::Right, ([1.0, 0.0, 0.0], [4.0, 4.0, 4.0])),
        (Keep::Left, ([0.0, 0.0, 0.0], [1.0, 4.0, 4.0])),
    ] {
        let (mut s, id) = one_box();
        s.set_tool(Tool::Slice);
        s.keep = keep;
        drag(&mut s, View::Top, [1.0, -1.0], [1.0, 5.0]);
        assert!(s.apply_slice());
        assert_eq!(s.doc.brushes().len(), 1);
        assert_eq!(bounds(&s, id), want, "{keep:?}");
    }
}

#[test]
fn slice_missing_the_brush_changes_nothing() {
    let (mut s, _) = one_box();
    s.set_tool(Tool::Slice);
    let before = s.doc.clone();
    drag(&mut s, View::Top, [9.0, -1.0], [9.0, 5.0]);
    assert!(!s.apply_slice());
    assert_eq!(s.doc.brushes(), before.brushes());
    assert_eq!(s.history.depth(), 1, "only the box was recorded");
}

#[test]
fn diagonal_slice_makes_wedges_and_escape_drops_the_line() {
    let (mut s, id) = one_box();
    s.set_tool(Tool::Slice);
    drag(&mut s, View::Top, [-1.0, -1.0], [5.0, 5.0]);
    s.cancel_drag();
    assert!(s.cut.is_none());
    assert!(!s.apply_slice());
    drag(&mut s, View::Top, [-1.0, -1.0], [5.0, 5.0]);
    assert!(s.apply_slice());
    assert_eq!(s.doc.brushes().len(), 2);
    for b in s.doc.brushes() {
        assert!(matches!(b.shape, Shape::Convex(_)), "a wedge is not a box");
        assert!(b.to_brush(0).is_ok());
    }
    assert_eq!(bounds(&s, id), ([0.0, 0.0, 0.0], [4.0, 4.0, 4.0]));
}

#[test]
fn carve_cuts_the_overlap_out_of_other_brushes() {
    let mut s = EditorState {
        new_op: Op::Add,
        ..EditorState::default()
    };
    let big = draw(&mut s, [0.0, 0.0], [10.0, 10.0]);
    let cutter = draw(&mut s, [4.0, 4.0], [6.0, 6.0]);
    let before = s.doc.clone();
    assert!(s.carve_selected());
    assert!(s.doc.brush(cutter).is_none(), "the cutter is consumed");
    assert!(s.doc.brush(big).is_some(), "the first piece keeps the id");
    assert!(s.doc.brushes().len() > 1);
    let total: f64 = s.doc.brushes().iter().map(volume).sum();
    assert!((total - (400.0 - 16.0)).abs() < 1e-6, "volume {total}");
    assert!(s.undo());
    assert_eq!(s.doc.brushes(), before.brushes());
}

#[test]
fn carve_with_nothing_overlapping_is_refused() {
    let mut s = EditorState::default();
    draw(&mut s, [0.0, 0.0], [2.0, 2.0]);
    draw(&mut s, [5.0, 5.0], [7.0, 7.0]);
    assert!(!s.carve_selected());
    assert_eq!(s.doc.brushes().len(), 2);
}

#[test]
fn carve_swallowing_a_brush_removes_it() {
    let mut s = EditorState::default();
    draw(&mut s, [1.0, 1.0], [2.0, 2.0]);
    draw(&mut s, [0.0, 0.0], [4.0, 4.0]);
    assert!(s.carve_selected());
    assert!(s.doc.brushes().is_empty());
    assert!(s.undo());
    assert_eq!(s.doc.brushes().len(), 2);
}

#[test]
fn dragging_a_corner_reshapes_the_brush() {
    let (mut s, id) = one_box();
    s.set_tool(Tool::Vertex);
    let before = s.doc.clone();
    // Both corners over (4, 4) in the top view move: a vertical edge.
    drag(&mut s, View::Top, [4.0, 4.0], [6.0, 4.0]);
    let moved = s.doc.brush(id).unwrap();
    assert_eq!(moved.bounds(), ([0.0, 0.0, 0.0], [6.0, 4.0, 4.0]));
    assert!(matches!(moved.shape, Shape::Convex(_)));
    assert!(moved.to_brush(0).is_ok());
    assert!(s.undo());
    assert_eq!(s.doc.brushes(), before.brushes());
}

#[test]
fn one_corner_dragged_in_the_front_view_tilts_the_top() {
    let (mut s, id) = one_box();
    s.set_tool(Tool::Vertex);
    // In the front view (x, z) the corner (4, ., 4) covers two vertices.
    drag(&mut s, View::Front, [4.0, 4.0], [4.0, 6.0]);
    assert_eq!(bounds(&s, id).1[2], 6.0);
    let n = s.doc.brush(id).unwrap().to_brush(0).unwrap().faces().len();
    assert!(n >= 6);
}

#[test]
fn a_corner_pushed_inside_the_brush_is_refused() {
    let (mut s, id) = one_box();
    s.set_tool(Tool::Vertex);
    let before = s.doc.clone();
    drag(&mut s, View::Top, [4.0, 4.0], [1.0, 1.0]);
    assert_eq!(s.doc.brushes(), before.brushes());
    assert_eq!(s.message, "A corner cannot go there");
    assert!(s.doc.brush(id).is_some());
    assert_eq!(s.history.depth(), 1, "only the box was recorded");
}

#[test]
fn escape_during_a_vertex_drag_restores_the_brush() {
    let (mut s, _) = one_box();
    s.set_tool(Tool::Vertex);
    let before = s.doc.clone();
    s.pointer_down(ptr(View::Top, 4.0, 4.0));
    s.pointer_move(ptr(View::Top, 6.0, 4.0));
    assert_ne!(s.doc.brushes(), before.brushes());
    s.cancel_drag();
    assert_eq!(s.doc.brushes(), before.brushes());
}

fn top_face(s: &EditorState, id: Id) -> FaceRef {
    let brush = s.doc.brush(id).unwrap().to_brush(0).unwrap();
    let plane = brush
        .plane_facing(mscent_math::Vec3d::new(0.0, 0.0, 1.0))
        .unwrap();
    FaceRef { brush: id, plane }
}

#[test]
fn extruding_a_face_adds_a_prism_and_follows_it() {
    let (mut s, id) = one_box();
    s.set_tool(Tool::Face);
    s.faces = vec![top_face(&s, id)];
    let before = s.doc.clone();
    assert!(s.extrude_faces(2.0));
    assert_eq!(s.doc.brushes().len(), 2);
    let new = s.doc.brushes()[1].clone();
    assert_eq!(new.bounds(), ([0.0, 0.0, 4.0], [4.0, 4.0, 6.0]));
    assert_eq!((new.op, new.material), (s.doc.brushes()[0].op, 0));
    // The picked face is now the prism's far face, ready to push again.
    assert_eq!(s.faces, vec![top_face(&s, new.id)]);
    assert!(s.extrude_faces(1.0));
    assert_eq!(s.doc.brushes().len(), 3);
    assert_eq!(s.doc.brushes()[2].bounds().1[2], 7.0);
    assert!(s.undo() && s.undo());
    assert_eq!(s.doc.brushes(), before.brushes());
}

#[test]
fn extruding_inward_moves_the_face() {
    let (mut s, id) = one_box();
    s.set_tool(Tool::Face);
    s.faces = vec![top_face(&s, id)];
    assert!(s.extrude_faces(-1.0));
    assert_eq!(s.doc.brushes().len(), 1);
    assert_eq!(bounds(&s, id), ([0.0, 0.0, 0.0], [4.0, 4.0, 3.0]));
    assert!(s.faces.is_empty(), "plane numbers may have changed");
    // Pulling past the opposite face leaves nothing: refused.
    s.faces = vec![top_face(&s, id)];
    assert!(!s.extrude_faces(-9.0));
    assert_eq!(bounds(&s, id).1[2], 3.0);
}

#[test]
fn extruding_without_faces_does_nothing() {
    let (mut s, _) = one_box();
    assert!(!s.extrude_faces(1.0));
    assert_eq!(s.doc.brushes().len(), 1);
}

#[test]
fn extrusion_keeps_the_face_texture_and_stays_inside_the_limit() {
    let (mut s, id) = one_box();
    s.set_tool(Tool::Face);
    s.new_material = 0;
    s.faces = vec![top_face(&s, id)];
    s.grid = 8.0;
    assert!(!s.extrude_faces(1000.0), "past LIMIT");
    assert_eq!(s.doc.brushes().len(), 1);
}

#[test]
fn switching_tools_drops_a_pending_slice() {
    let (mut s, _) = one_box();
    s.set_tool(Tool::Slice);
    drag(&mut s, View::Top, [1.0, -1.0], [1.0, 5.0]);
    assert!(s.slice_line().is_some());
    s.set_tool(Tool::Select);
    assert!(s.slice_line().is_none());
}

#[test]
fn one_sided_slice_that_misses_is_refused() {
    for keep in [Keep::Left, Keep::Right] {
        let (mut s, _) = one_box();
        s.set_tool(Tool::Slice);
        s.keep = keep;
        // Entirely to the right of the box, drawn along +y.
        drag(&mut s, View::Top, [9.0, -1.0], [9.0, 5.0]);
        assert!(!s.apply_slice(), "{keep:?}");
        assert_eq!(s.history.depth(), 1);
    }
}

#[test]
fn tools_keep_each_faces_own_material() {
    let (mut s, id) = one_box();
    let mut def = s.doc.brush(id).unwrap().clone();
    let mut infos = def.full_texinfos();
    // The floor survives both halves of a cut.
    let floor = def
        .to_brush(0)
        .unwrap()
        .plane_facing(mscent_math::Vec3d::new(0.0, 0.0, -1.0))
        .unwrap();
    infos[floor].material = 3;
    def.texinfos = infos;
    s.doc.replace_brush_for_test(&def);
    let painted = |b: &BrushDef| b.full_texinfos().iter().any(|t| t.material == 3);
    s.set_tool(Tool::Slice);
    drag(&mut s, View::Top, [2.0, -1.0], [2.0, 5.0]);
    s.keep = Keep::Both;
    assert!(s.apply_slice());
    assert!(s.doc.brushes().iter().any(painted), "slice lost the paint");
    s.set_tool(Tool::Vertex);
    s.selection = Some(Selection::Brush(id));
    drag(&mut s, View::Top, [0.0, 0.0], [-1.0, 0.0]);
    assert!(
        painted(s.doc.brush(id).unwrap()),
        "vertex move lost the paint"
    );
}

#[test]
fn edits_that_do_nothing_say_why() {
    let (mut s, _) = one_box();
    assert!(!s.reorder_selected(1));
    assert_eq!(s.message, "Already last in order");
    assert!(!s.reorder_selected(-1));
    assert_eq!(s.message, "Already first in order");
    s.set_tool(Tool::Slice);
    drag(&mut s, View::Top, [2.0, 2.0], [2.0, 2.0]);
    assert!(
        s.message.starts_with("The slice line has no length"),
        "{}",
        s.message
    );
    s.place_entity(EntityKind::PlayerStart, [1.0, 1.0, 1.0]);
    assert_eq!(s.message, "Placed player start at 1, 1, 1");
    s.place_entity(EntityKind::PlayerStart, [2.0, 2.0, 1.0]);
    assert!(
        s.message.starts_with("Moved the player start"),
        "{}",
        s.message
    );
}

#[test]
fn a_drag_past_the_map_stops_at_the_edge_and_says_so() {
    let (mut s, id) = one_box();
    s.set_tool(Tool::Select);
    drag(&mut s, View::Top, [2.0, 2.0], [5000.0, 2.0]);
    let (lo, hi) = bounds(&s, id);
    assert!(hi[0] <= 4096.0 && lo[0] >= 0.0, "{lo:?} {hi:?}");
    assert!(
        s.message.starts_with("That would leave the map"),
        "{}",
        s.message
    );
}

#[test]
fn counts_read_as_words() {
    assert_eq!(super::count(1, "brush", "brushes"), "1 brush");
    assert_eq!(super::count(3, "face", "faces"), "3 faces");
}

#[test]
fn a_view_takes_the_coordinate_it_hides_from_the_last_click_elsewhere() {
    let mut s = EditorState::default();
    // A click in the side view sets Y and Z; the top view then sets X and Y.
    s.set_tool(Tool::Select);
    s.pointer_down(ptr(View::Side, 3.0, 6.0));
    s.pointer_up(ptr(View::Side, 3.0, 6.0));
    s.set_tool(Tool::Place(EntityKind::Light));
    s.pointer_down(ptr(View::Top, 1.0, 2.0));
    let light = s.selected_entity().expect("placed").entity;
    assert_eq!(super::entity::pos(&light), [1.0, 2.0, 6.0]);
    assert_eq!(s.message, "Placed light at 1, 2, 6");
}

#[test]
fn a_box_drawn_with_a_brush_selected_spans_its_depth() {
    let (mut s, _) = one_box();
    let (lo, hi) = s.selected_brush().expect("selected").bounds();
    s.set_tool(Tool::Box);
    drag(&mut s, View::Front, [1.0, 1.0], [2.0, 3.0]);
    let (nlo, nhi) = s.selected_brush().expect("new box").bounds();
    assert_eq!((nlo[1], nhi[1]), (lo[1], hi[1]));
}
