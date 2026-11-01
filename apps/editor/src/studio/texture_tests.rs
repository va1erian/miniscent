//! Texture edits reuse the cached lightmap: the baker does not run again,
//! and the new mapping shows without a manual build.

use std::time::{Duration, Instant};

use mscent_render::Source;
use mscent_world::Op;

use super::*;
use crate::build::{SharedCache, Update, start_cached};
use crate::model::facetex::FaceEdit;
use crate::model::{EditorState, Selection};

const PANE: (u32, u32) = (480, 300);

fn showcase() -> Document {
    Document::from_level(&mscent_world::showcase_level())
}

/// The showcase with every face of its first brush remapped.
fn remapped(doc: &Document) -> Document {
    let mut st = EditorState::new(doc.clone());
    st.selection = Some(Selection::Brush(doc.brushes()[0].id));
    assert!(st.apply_face_edit(FaceEdit::ScaleU(0.25)));
    assert!(st.apply_face_edit(FaceEdit::Rotation(45.0)));
    st.doc
}

fn finish(job: &mut crate::build::BuildJob) -> crate::build::Built {
    loop {
        for u in job.poll() {
            if let Update::Finished(r) = u {
                return r.expect("builds");
            }
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn a_new_mapping_does_not_bake_again_but_geometry_does() {
    let doc = showcase();
    let cache = SharedCache::default();
    let first = finish(&mut start_cached(&doc, false, &cache).expect("starts"));
    assert_eq!(first.bake, Some(Source::Baked));
    let before: Vec<_> = first
        .renderer()
        .scene()
        .faces()
        .iter()
        .map(|f| f.texinfo)
        .collect();

    let edited = remapped(&doc);
    assert!(edited.same_geometry(&doc) && edited != doc);
    let second = finish(&mut start_cached(&edited, false, &cache).expect("starts"));
    assert_eq!(second.bake, Some(Source::Memory), "lightmap kept");
    assert_eq!(
        cache.lock().expect("cache").bakes(),
        1,
        "the baker ran once"
    );
    let after: Vec<_> = second
        .renderer()
        .scene()
        .faces()
        .iter()
        .map(|f| f.texinfo)
        .collect();
    assert_ne!(before, after, "the UVs were refreshed");
    assert_eq!(
        first.renderer().lightmap().to_bytes(0),
        second.renderer().lightmap().to_bytes(0),
        "identical lightmap data"
    );

    // Moving a brush is geometry: that one has to bake.
    let mut st = EditorState::new(doc.clone());
    st.selection = Some(Selection::Brush(doc.brushes()[0].id));
    st.new_op = Op::Add;
    st.add_box([40.0, 40.0, 0.0], [42.0, 42.0, 2.0])
        .expect("added");
    assert!(!st.doc.same_geometry(&doc));
    let third = finish(&mut start_cached(&st.doc, false, &cache).expect("starts"));
    assert_eq!(third.bake, Some(Source::Baked));
    assert_eq!(cache.lock().expect("cache").bakes(), 2);
}

#[test]
fn the_studio_refreshes_a_texture_edit_in_the_background_without_baking() {
    let doc = showcase();
    let mut studio = Studio::new(&doc);
    studio.start_build(&doc, false).expect("starts");
    while studio.building() {
        studio.tick(&[], PANE, &doc);
        std::thread::sleep(Duration::from_millis(5));
    }
    let cache = studio.cache();
    assert_eq!(cache.lock().expect("cache").bakes(), 1);

    let edited = remapped(&doc);
    assert!(studio.stale(&edited));
    studio.note_edit();
    let end = Instant::now() + Duration::from_secs(60);
    let mut log = Vec::new();
    let mut status = None;
    while studio.stale(&edited) || studio.building() || status.is_none() {
        let t = studio.tick(&[], PANE, &edited);
        log.extend(t.log);
        status = status.or(t.status);
        assert!(Instant::now() < end, "never refreshed");
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(log.is_empty(), "a refresh is quiet: {log:?}");
    let status = status.expect("status");
    assert!(
        status.starts_with("Texture mapping updated") && status.contains("lightmap kept"),
        "{status}"
    );
    assert_eq!(cache.lock().expect("cache").bakes(), 1, "no bake ran");

    // A geometry edit is left for the user to build.
    let mut st = EditorState::new(edited.clone());
    st.new_op = Op::Add;
    st.add_box([40.0, 40.0, 0.0], [42.0, 42.0, 2.0])
        .expect("added");
    studio.note_edit();
    for _ in 0..20 {
        studio.tick(&[], PANE, &st.doc);
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(!studio.building() && studio.stale(&st.doc));
}
