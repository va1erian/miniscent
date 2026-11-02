//! Ray picking and the texture edits behind the face panel.

use mscent_math::{Vec3, Vec3d};
use mscent_render::View;
use mscent_world::Op;
use mscent_world::texinfo::Justify;

use super::document::Id;
use super::facetex::FaceEdit;
use super::pick::{FaceRef, Ray, pick_face, view_ray};
use super::state::{EditorState, Selection, Tool};

fn ray(origin: [f64; 3], dir: [f64; 3]) -> Ray {
    Ray {
        origin: Vec3d::new(origin[0], origin[1], origin[2]),
        dir: Vec3d::new(dir[0], dir[1], dir[2]),
    }
}

/// A carved room, x and y in -4..4, z in 0..3. Returns its id.
fn room(st: &mut EditorState) -> Id {
    st.new_op = Op::Subtract;
    st.add_box([-4.0, -4.0, 0.0], [4.0, 4.0, 3.0])
        .expect("room")
}

/// The face of `brush` whose outward normal is `n`.
fn plane_facing(st: &EditorState, brush: Id, n: [f64; 3]) -> FaceRef {
    let i = st.doc.brush_index(brush).expect("brush");
    let b = st.doc.brushes()[i].to_brush(i as u32).expect("valid");
    let want = Vec3d::new(n[0], n[1], n[2]);
    let plane = b
        .planes()
        .iter()
        .position(|p| (p.normal - want).length() < 1e-9)
        .expect("plane");
    FaceRef { brush, plane }
}

#[test]
fn a_ray_hits_the_inside_faces_of_a_carved_room() {
    let mut st = EditorState::default();
    let id = room(&mut st);
    let wall = pick_face(&st.doc, &ray([0.0, -3.0, 1.5], [0.0, 1.0, 0.0])).expect("wall");
    assert_eq!(wall.face, plane_facing(&st, id, [0.0, 1.0, 0.0]));
    assert!((wall.t - 7.0).abs() < 1e-9 && (wall.point.y - 4.0).abs() < 1e-9);
    let floor = pick_face(&st.doc, &ray([0.0, 0.0, 1.5], [0.0, 0.0, -1.0])).expect("floor");
    assert_eq!(floor.face, plane_facing(&st, id, [0.0, 0.0, -1.0]));
    assert!((floor.t - 1.5).abs() < 1e-9);
    // A zero direction and a ray leaving the level upwards find nothing.
    assert!(pick_face(&st.doc, &ray([0.0, 0.0, 1.5], [0.0, 0.0, 0.0])).is_none());
    assert!(pick_face(&st.doc, &ray([0.0, 0.0, 10.0], [0.0, 0.0, 1.0])).is_none());
}

#[test]
fn faces_behind_others_and_buried_faces_are_not_picked() {
    let mut st = EditorState::default();
    let id = room(&mut st);
    st.new_op = Op::Add;
    let pillar = st
        .add_box([-1.0, 0.0, 0.0], [1.0, 1.0, 3.0])
        .expect("pillar");
    // The pillar hides the wall behind it, and only its visible sides count.
    let front = pick_face(&st.doc, &ray([0.0, -3.0, 1.5], [0.0, 1.0, 0.0])).expect("pillar");
    assert_eq!(front.face, plane_facing(&st, pillar, [0.0, -1.0, 0.0]));
    assert!((front.t - 3.0).abs() < 1e-9);
    let back = pick_face(&st.doc, &ray([0.0, 3.0, 1.5], [0.0, -1.0, 0.0])).expect("pillar back");
    assert_eq!(back.face, plane_facing(&st, pillar, [0.0, 1.0, 0.0]));
    // Beside the pillar the wall is visible again.
    let wall = pick_face(&st.doc, &ray([2.0, -3.0, 1.5], [0.0, 1.0, 0.0])).expect("wall");
    assert_eq!(wall.face, plane_facing(&st, id, [0.0, 1.0, 0.0]));
    // A slab sunk into the north wall buries the part of the wall it covers.
    let slab = st.add_box([-1.0, 3.0, 0.0], [1.0, 5.0, 3.0]).expect("slab");
    let hit = pick_face(&st.doc, &ray([0.5, 0.5, 1.5], [0.0, 1.0, 0.0])).expect("slab face");
    assert_eq!(hit.face, plane_facing(&st, slab, [0.0, -1.0, 0.0]));
    assert!((hit.point.y - 3.0).abs() < 1e-9);
}

#[test]
fn view_rays_follow_the_camera() {
    let view = View::from_angles(Vec3::new(1.0, 2.0, 3.0), 0.0, 0.0, 0.0);
    let centre = view_ray(&view, 1.3, 0.5, 0.5, 1.5).expect("ray");
    assert!((centre.dir.y - 1.0).abs() < 1e-6 && centre.dir.x.abs() < 1e-6);
    assert_eq!(centre.origin, Vec3d::new(1.0, 2.0, 3.0));
    let right = view_ray(&view, 1.3, 1.0, 0.5, 1.5).expect("ray");
    let up = view_ray(&view, 1.3, 0.5, 0.0, 1.5).expect("ray");
    assert!(right.dir.x > 0.5 && up.dir.z > 0.3, "{right:?} {up:?}");
    assert!(view_ray(&view, 1.3, f32::NAN, 0.5, 1.5).is_none());
    assert!(view_ray(&view, 1.3, 0.5, 0.5, 0.0).is_none());
}

#[test]
fn picking_selects_faces_and_ctrl_toggles_more() {
    let mut st = EditorState::default();
    let id = room(&mut st);
    st.tool = Tool::Face;
    let floor_ray = ray([0.0, 0.0, 1.5], [0.0, 0.0, -1.0]);
    let wall_ray = ray([0.0, -3.0, 1.5], [0.0, 1.0, 0.0]);
    st.selection = None;
    st.pick_face_at(&floor_ray, false);
    assert_eq!(st.faces, vec![plane_facing(&st, id, [0.0, 0.0, -1.0])]);
    assert_eq!(st.selection, Some(Selection::Brush(id)));
    st.pick_face_at(&wall_ray, true);
    assert_eq!(st.faces.len(), 2);
    assert_eq!(st.highlighted_faces().len(), 2);
    st.pick_face_at(&wall_ray, true);
    assert_eq!(st.faces.len(), 1, "ctrl-click again unpicks");
    st.pick_face_at(&ray([0.0, 0.0, 10.0], [0.0, 0.0, 1.0]), false);
    assert!(st.faces.is_empty(), "clicking nothing clears");
    // With nothing picked the selected brush's six faces are the targets.
    assert_eq!(st.target_faces().len(), 6);
    st.tool = Tool::Select;
    assert!(st.highlighted_faces().is_empty());
}

fn floor_state() -> (EditorState, FaceRef) {
    let mut st = EditorState::default();
    let id = room(&mut st);
    st.tool = Tool::Face;
    st.pick_face_at(&ray([0.0, 0.0, 1.5], [0.0, 0.0, -1.0]), false);
    let floor = st.faces[0];
    assert_eq!(floor, plane_facing(&st, id, [0.0, 0.0, -1.0]));
    (st, floor)
}

#[test]
fn texture_edits_apply_undo_and_redo() {
    let (mut st, floor) = floor_state();
    let before = st.face_texinfo(floor).expect("info");
    assert!(st.apply_face_edit(FaceEdit::ScaleU(0.125)));
    let after = st.face_texinfo(floor).expect("info");
    assert_eq!((after.u_scale, after.v_scale), (0.125, before.v_scale));
    assert!(st.apply_face_edit(FaceEdit::Rotation(45.0)));
    assert!(st.apply_face_edit(FaceEdit::ShiftV(12.0)));
    let edited = st.face_texinfo(floor).expect("info");
    assert_eq!((edited.rotation_deg, edited.v_shift), (45.0, 12.0));
    // Other faces of the brush keep their defaults.
    let other = FaceRef {
        plane: (floor.plane + 1) % 6,
        ..floor
    };
    assert_eq!(
        st.face_texinfo(other).expect("info").u_scale,
        before.u_scale
    );
    for _ in 0..3 {
        assert!(st.undo());
    }
    assert_eq!(st.face_texinfo(floor), Some(before));
    for _ in 0..3 {
        assert!(st.redo());
    }
    assert_eq!(st.face_texinfo(floor), Some(edited));
    // Nothing to change is not an undo step.
    let depth = st.history.depth();
    assert!(!st.apply_face_edit(FaceEdit::Rotation(45.0)));
    assert_eq!(st.history.depth(), depth);
}

#[test]
fn consecutive_edits_of_one_field_undo_in_one_step() {
    let (mut st, floor) = floor_state();
    let original = st.face_texinfo(floor).expect("info");
    for s in [0.1, 0.2, 0.3, 0.4] {
        st.apply_face_edit(FaceEdit::ScaleV(s));
    }
    assert_eq!(st.history.depth(), 2, "room plus one merged edit");
    assert_eq!(st.face_texinfo(floor).expect("info").v_scale, 0.4);
    st.apply_face_edit(FaceEdit::ShiftU(3.0));
    assert_eq!(st.history.depth(), 3, "another field starts a new step");
    st.undo();
    st.undo();
    assert_eq!(st.face_texinfo(floor), Some(original));
    // After an undo a new edit is not folded into the older one.
    st.apply_face_edit(FaceEdit::ScaleV(0.5));
    st.apply_face_edit(FaceEdit::ScaleV(0.6));
    assert_eq!(st.history.depth(), 2);
    st.undo();
    assert_eq!(st.face_texinfo(floor), Some(original));
}

#[test]
fn fit_justify_flip_and_material_use_the_picked_faces() {
    let (mut st, floor) = floor_state();
    st.texture_sizes = vec![(128, 128); 5];
    assert!(st.apply_face_edit(FaceEdit::Fit {
        tiles_u: 2.0,
        tiles_v: 4.0
    }));
    let v = st.face_view().expect("view");
    assert_eq!(v.count, 1);
    // 8 units across 2 repeats of a 128 texel texture.
    assert!((v.info.u_scale - 8.0 / 256.0).abs() < 1e-6, "{:?}", v.info);
    assert!((v.info.v_scale - 8.0 / 512.0).abs() < 1e-6);
    assert!((v.tile.0 - 4.0).abs() < 1e-4 && (v.tile.1 - 2.0).abs() < 1e-4);
    assert!(st.apply_face_edit(FaceEdit::Justify(Justify::Center)));
    assert!(st.apply_face_edit(FaceEdit::FlipU));
    assert!(st.face_texinfo(floor).expect("info").u_scale < 0.0);
    assert!(st.apply_face_edit(FaceEdit::Material(3)));
    let i = st.doc.brush_index(floor.brush).expect("brush");
    let built = st.doc.brushes()[i].to_brush(0).expect("valid");
    assert_eq!(built.texinfos()[floor.plane].material, 3);
    let others = built.texinfos().iter().filter(|t| t.material != 3).count();
    assert_eq!(others, 5, "only the picked face changed material");
    assert!(st.apply_face_edit(FaceEdit::Rotation(30.0)));
    assert!(st.apply_face_edit(FaceEdit::AlignFace));
    assert_eq!(st.face_texinfo(floor).expect("info").rotation_deg, 0.0);
    assert!(st.apply_face_edit(FaceEdit::Rotation(30.0)));
    assert!(st.apply_face_edit(FaceEdit::AlignWorld));
    assert_eq!(st.face_texinfo(floor).expect("info").rotation_deg, 0.0);
}

#[test]
fn a_selected_brush_edits_all_its_faces_at_once() {
    let mut st = EditorState::default();
    room(&mut st);
    assert!(st.faces.is_empty());
    assert!(st.apply_face_edit(FaceEdit::Material(2)));
    let b = &st.doc.brushes()[0];
    assert!(b.texinfos.iter().all(|t| t.material == 2) && b.texinfos.len() == 6);
    assert_eq!(b.material, 2, "the brush material follows when all agree");
    assert!(st.apply_face_edit(FaceEdit::ScaleU(0.5)));
    assert!(
        st.doc.brushes()[0]
            .texinfos
            .iter()
            .all(|t| t.u_scale == 0.5)
    );
    st.undo();
    st.undo();
    assert_eq!(st.doc.brushes()[0].material, 0);
}

#[test]
fn copying_a_face_carries_the_alignment_across_a_shared_edge() {
    let (mut st, floor) = floor_state();
    st.apply_face_edit(FaceEdit::Material(3));
    st.apply_face_edit(FaceEdit::ScaleU(0.25));
    assert!(st.copy_face(floor));
    let wall = plane_facing(&st, floor.brush, [0.0, 1.0, 0.0]);
    assert!(st.paste_onto(&[wall]));
    let t = st.face_texinfo(wall).expect("info");
    assert_eq!((t.material, t.u_scale), (3, 0.25));
    // Unfolded across the shared edge, the floor x axis is still x.
    assert!((t.u_axis.x.abs() - 1.0).abs() < 1e-5, "{:?}", t.u_axis);
    assert!(st.undo());
    assert_ne!(st.face_texinfo(wall).expect("info").material, 3);
    let mut empty = EditorState::default();
    assert!(!empty.paste_onto(&[floor]), "nothing copied yet");
}

#[test]
fn texture_lock_off_moves_the_texture_with_the_brush() {
    let (mut st, floor) = floor_state();
    let start = st.face_texinfo(floor).expect("info");
    // Locked to the world (the default): moving the brush leaves the shift.
    st.edit_selected_brush(|b| b.translate([1.0, 0.0, 0.0]));
    assert_eq!(st.face_texinfo(floor).expect("info").u_shift, start.u_shift);
    st.apply_face_edit(FaceEdit::FollowBrush(true));
    let before = st.face_texinfo(floor).expect("info");
    st.edit_selected_brush(|b| b.translate([1.0, 0.0, 0.0]));
    let after = st.face_texinfo(floor).expect("info");
    assert!((after.u_shift - (before.u_shift - 1.0 / before.u_scale)).abs() < 1e-3);
}

#[test]
fn edited_mappings_survive_save_and_load() {
    let (mut st, floor) = floor_state();
    st.apply_face_edit(FaceEdit::ScaleU(0.5));
    st.apply_face_edit(FaceEdit::Rotation(45.0));
    st.apply_face_edit(FaceEdit::ShiftV(7.0));
    st.apply_face_edit(FaceEdit::FlipV);
    st.apply_face_edit(FaceEdit::Material(4));
    st.apply_face_edit(FaceEdit::FollowBrush(true));
    let expected = st.face_texinfo(floor).expect("info");
    let path = std::env::temp_dir().join(format!("mscent_facetex_{}.msl", std::process::id()));
    crate::files::save(&st.doc, &path).expect("saves");
    let loaded = crate::files::load(&path).expect("loads");
    let _ = std::fs::remove_file(&path);
    let mut back = EditorState::new(loaded);
    back.selection = Some(Selection::Brush(back.doc.brushes()[0].id));
    let got = back.face_texinfo(floor).expect("info");
    assert_eq!(got, expected);
    assert_eq!(
        (got.rotation_deg, got.v_shift, got.material),
        (45.0, -7.0, 4)
    );
    assert!(!got.lock_to_world);
    // The other faces are untouched by the round trip.
    let all = back.doc.brushes()[0].full_texinfos();
    assert_eq!(all.iter().filter(|t| t.material == 4).count(), 1);
}

#[test]
fn the_select_tool_picks_the_brush_under_a_view_ray() {
    let mut st = EditorState::default();
    let room = room(&mut st);
    assert!(
        st.select_brush_at(&ray([0.0, 0.0, 1.5], [0.0, 0.0, -1.0]))
            .is_some()
    );
    assert_eq!(st.selection, Some(Selection::Brush(room)));
    // Looking out of the level hits nothing and clears the selection.
    assert!(
        st.select_brush_at(&ray([0.0, 0.0, 10.0], [0.0, 0.0, 1.0]))
            .is_none()
    );
    assert_eq!(st.selection, None);
}
