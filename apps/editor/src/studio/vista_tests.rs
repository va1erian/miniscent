//! A level with vistas through the preview path the 3D pane uses: the warnings
//! reach the log once, and a level whose every face is a view or self-lit still
//! gets a picture and can be played.

use mscent_world::materials::view_material;

use super::preview_tests::{settle, tick_until};
use super::*;
use crate::model::facetex::FaceEdit;
use crate::model::pick::FaceRef;
use crate::model::{EditorState, EntityKind};

/// A play room of 16 by 16 by 8 with a start and a goal.
fn play_room() -> EditorState {
    let mut st = EditorState::default();
    st.add_box([0.0; 3], [16.0, 16.0, 8.0]).expect("room");
    st.place_entity(EntityKind::PlayerStart, [8.0, 8.0, 2.0]);
    st.place_entity(EntityKind::Goal, [12.0, 12.0, 1.0]);
    st
}

/// Paints every plane of the first brush with `slot`'s view, as the face panel does.
fn paint_all_views(st: &mut EditorState, slot: u8) {
    let brush = st.doc.brushes()[0].id;
    st.faces = (0..6).map(|plane| FaceRef { brush, plane }).collect();
    let view = view_material(slot).expect("a slot");
    assert!(st.apply_face_edit(FaceEdit::Material(view)));
    st.faces.clear();
}

#[test]
fn a_wall_with_no_layer_is_warned_of_once_and_the_warning_goes_with_the_fix() {
    let mut st = play_room();
    paint_all_views(&mut st, 0);
    let mut studio = Studio::new(&st.doc);
    let watch = settle(&mut studio, &st.doc);
    let said: Vec<_> = watch.log.iter().filter(|l| l.contains("WARNING")).collect();
    assert_eq!(said.len(), 1, "{:?}", watch.log);
    assert!(said[0].contains("view 0"), "{said:?}");
    // An edit that leaves the vistas alone does not repeat it.
    st.place_entity(EntityKind::Light, [4.0, 4.0, 4.0]);
    let watch = settle(&mut studio, &st.doc);
    assert!(
        !watch.log.iter().any(|l| l.contains("WARNING")),
        "{:?}",
        watch.log
    );
    // The fix: New Vista feeds slot 0.
    assert!(st.new_vista());
    let watch = settle(&mut studio, &st.doc);
    assert!(
        !watch.log.iter().any(|l| l.contains("WARNING")),
        "{:?}",
        watch.log
    );
}

#[test]
fn a_level_of_only_views_and_self_lit_faces_has_a_picture_and_plays() {
    let mut st = play_room();
    paint_all_views(&mut st, 0);
    assert!(st.new_vista());
    let mut studio = Studio::new(&st.doc);
    let watch = settle(&mut studio, &st.doc);
    assert_eq!(studio.preview(&st.doc), Preview::Full, "{:?}", watch.log);
    assert!(
        !watch.log.iter().any(|l| l.contains("ERROR")),
        "{:?}",
        watch.log
    );
    assert!(studio.playable_now(&st.doc));
    studio.enter_play().expect("plays");
    let watch = tick_until(&mut studio, &st.doc, |s| s.mode == Mode::Play);
    assert!(
        !watch.log.iter().any(|l| l.contains("ERROR")),
        "{:?}",
        watch.log
    );
}
