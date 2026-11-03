//! Hostile input to the texture edits: extreme and non-finite values from a
//! number field, and level files with flipped bytes that still load.

use mscent_world::format;
use mscent_world::texinfo::Justify;

use super::document::Document;
use super::facetex::FaceEdit;
use super::state::{EditorState, Tool};

fn edits() -> Vec<FaceEdit> {
    let odd = [
        0.0,
        -0.0,
        f32::NAN,
        f32::INFINITY,
        f32::NEG_INFINITY,
        1e30,
        -1e30,
        1e-30,
    ];
    let mut v = vec![
        FaceEdit::Material(0),
        FaceEdit::Material(u16::MAX),
        FaceEdit::Justify(Justify::Center),
        FaceEdit::AlignFace,
        FaceEdit::AlignWorld,
        FaceEdit::FlipU,
        FaceEdit::FlipV,
        FaceEdit::FollowBrush(true),
        FaceEdit::FollowBrush(false),
    ];
    for x in odd {
        v.extend([
            FaceEdit::ScaleU(x),
            FaceEdit::ScaleV(x),
            FaceEdit::ShiftU(x),
            FaceEdit::ShiftV(x),
            FaceEdit::Rotation(x),
            FaceEdit::Fit {
                tiles_u: x,
                tiles_v: 1.0,
            },
            FaceEdit::Fit {
                tiles_u: 1.0,
                tiles_v: x,
            },
        ]);
    }
    v
}

/// Every edit on every face of every brush, then undo everything.
fn hammer(st: &mut EditorState) {
    st.tool = Tool::Face;
    st.texture_sizes = vec![(0, 0), (1, 1), (128, 128), (u32::MAX, 1)];
    for i in 0..st.doc.brushes().len() {
        let id = st.doc.brushes()[i].id;
        st.selection = Some(super::state::Selection::Brush(id));
        for e in edits() {
            st.apply_face_edit(e);
            let _ = st.face_view();
            let _ = st.highlights();
            for f in st.target_faces() {
                st.copy_face(f);
                st.paste_onto(&[f]);
            }
        }
    }
    while st.undo() {}
}

#[test]
fn extreme_and_non_finite_values_never_panic_or_corrupt_the_document() {
    let doc = Document::from_level(&mscent_world::showcase_level());
    let mut st = EditorState::new(doc.clone());
    hammer(&mut st);
    assert_eq!(st.doc, doc, "undoing everything restores the document");
    // Whatever the edits did, the level still saves and loads back.
    let mut st = EditorState::new(doc);
    for e in edits() {
        st.selection = st
            .doc
            .brushes()
            .first()
            .map(|b| super::state::Selection::Brush(b.id));
        st.apply_face_edit(e);
    }
    let level = st.doc.to_level().expect("still valid");
    let back = format::read(&format::write(&level)).expect("reads back");
    assert_eq!(back.brushes.len(), level.brushes.len());
}

#[test]
fn flipped_bytes_that_still_load_survive_every_edit() {
    let good = format::write(&mscent_world::showcase_level());
    let mut loaded = 0;
    for i in (0..good.len()).step_by(53) {
        let mut bad = good.clone();
        bad[i] ^= 0x5a;
        let Ok(level) = format::read(&bad) else {
            continue;
        };
        loaded += 1;
        let mut st = EditorState::new(Document::from_level(&level));
        st.doc
            .brushes()
            .first()
            .map(|b| b.id)
            .into_iter()
            .for_each(|id| {
                st.selection = Some(super::state::Selection::Brush(id));
            });
        for e in edits() {
            st.apply_face_edit(e);
            let _ = st.face_view();
        }
    }
    assert!(
        loaded > 5,
        "only {loaded} corrupt files loaded; the test is too weak"
    );
}
