//! Texture edits reuse the cached lightmap: the baker does not run again, and
//! the lightmap file follows the level through Save and the next session.

use std::time::{Duration, Instant};

use mscent_render::Source;
use mscent_world::Op;

use super::*;
use crate::build::{SharedCache, start_bake, start_preview};
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

fn finish<T>(job: &Job<T>) -> T {
    loop {
        if let Some(r) = job.poll() {
            return r.expect("builds");
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn preview_of(doc: &Document, cache: &SharedCache) -> Built {
    finish(&start_preview(doc, cache).expect("starts"))
}

fn uvs(built: &Built) -> Vec<mscent_world::TexInfo> {
    built
        .renderer()
        .scene()
        .faces()
        .iter()
        .map(|f| f.texinfo)
        .collect()
}

#[test]
fn a_new_mapping_finds_the_bake_but_new_geometry_does_not() {
    let doc = showcase();
    let cache = SharedCache::default();
    let draft = preview_of(&doc, &cache);
    assert_eq!(draft.light, Light::Draft);
    let baked = start_bake(&doc, draft.light_key, &cache).expect("starts");
    let report = finish(&baked.job).expect("not cancelled");
    assert_eq!(report.source, Source::Baked);
    let first = preview_of(&doc, &cache);
    assert_eq!(first.light, Light::Full);

    let edited = remapped(&doc);
    assert!(edited.same_geometry(&doc) && edited != doc);
    let second = preview_of(&edited, &cache);
    assert_eq!(second.light, Light::Full, "lightmap kept");
    assert_eq!(second.report.expect("found").source, Source::Memory);
    assert_eq!(
        cache.lock().expect("cache").bakes(),
        1,
        "the baker ran once"
    );
    let second = preview_of(&edited, &cache);
    assert_ne!(uvs(&first), uvs(&second), "the UVs were refreshed");
    assert_eq!(
        first.renderer().lightmap().to_bytes(0),
        second.renderer().lightmap().to_bytes(0),
        "identical lightmap data"
    );

    // Moving a brush is geometry: that one falls back to a draft.
    let mut st = EditorState::new(doc.clone());
    st.selection = Some(Selection::Brush(doc.brushes()[0].id));
    st.new_op = Op::Add;
    st.add_box([40.0, 40.0, 0.0], [42.0, 42.0, 2.0])
        .expect("added");
    assert!(!st.doc.same_geometry(&doc));
    let third = preview_of(&st.doc, &cache);
    assert_eq!(third.light, Light::Draft);
    assert_ne!(third.light_key, first.light_key);
    assert_eq!(cache.lock().expect("cache").bakes(), 1);
}

/// Ticks until `doc` is fully lit and returns the log lines that produced.
fn settle_log(studio: &mut Studio, doc: &Document) -> Vec<String> {
    let mut log = Vec::new();
    let end = Instant::now() + Duration::from_secs(60);
    while studio.preview(doc) != Preview::Full {
        log.extend(studio.tick(&[], PANE, doc).log);
        assert!(Instant::now() < end, "never settled");
        std::thread::sleep(Duration::from_millis(5));
    }
    log
}

fn temp_lit_path_exists(level: &std::path::Path) -> bool {
    miniscent_game::lightcache::path_for(Some(level), "x").is_some_and(|p| p.is_file())
}

fn temp_lit(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("mscent_editor_lit_{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("dir");
    let path = dir.join(name);
    let _ = std::fs::remove_file(&path);
    path
}

#[test]
fn a_saved_level_reopens_from_its_cache_and_play_never_bakes() {
    let doc = showcase();
    let level = temp_lit("level.msl");
    let lit = temp_lit("level.msl.lit");

    // First session: the bake happens by itself and writes the file next to
    // the level.
    let mut studio = Studio::new(&doc);
    studio.set_cache_level(Some(&level));
    let log = settle_log(&mut studio, &doc);
    let last = log.last().expect("log");
    assert!(
        last.starts_with("Lightmap baked in") && last.contains("written to"),
        "{last}"
    );
    assert!(lit.is_file());
    let bakes = studio.cache().lock().expect("cache").bakes();
    assert_eq!(bakes, 1);

    // Instant Play uses the picture on screen: no bake.
    studio.enter_play().expect("plays");
    assert_eq!(studio.cache().lock().expect("cache").bakes(), 1);
    studio.leave_play();

    // A texture edit keeps the bake; Save writes the file for the saved path.
    let edited = remapped(&doc);
    let moved = temp_lit("moved.msl");
    let note = studio.save_lightmap(&moved, &edited).expect("saved");
    assert!(note.starts_with("lightmap cached in"), "{note}");
    assert!(temp_lit_path_exists(&moved));

    // Second session, same level: loaded from the cache, zero bakes, and the
    // very first picture is already fully lit.
    let mut again = Studio::new(&doc);
    again.set_cache_level(Some(&level));
    let log = settle_log(&mut again, &doc);
    let last = log.last().expect("log");
    assert!(last.starts_with("Lightmap loaded from cache"), "{last}");
    assert_eq!(again.cache().lock().expect("cache").bakes(), 0);

    // Editing a texture still does not bake; editing a light does.
    let log = settle_log(&mut again, &edited);
    assert!(log.is_empty(), "{log:?}");
    assert_eq!(again.cache().lock().expect("cache").bakes(), 0);
    let mut lit_doc = showcase();
    let id = lit_doc.alloc_id();
    lit_doc.insert_entity_for_test(
        id,
        mscent_world::Entity::Light {
            pos: mscent_math::Vec3::new(5.0, 5.0, 3.0),
            color: mscent_math::Vec3::new(1.0, 0.5, 0.5),
            radius: 10.0,
            intensity: 1.0,
        },
    );
    let log = settle_log(&mut again, &lit_doc);
    let last = log.last().expect("log");
    assert!(
        last.starts_with("Lightmap baked in") && last.contains("stale"),
        "{last}"
    );
    assert_eq!(again.cache().lock().expect("cache").bakes(), 1);
}

#[test]
fn saving_on_draft_light_writes_no_file_and_the_coming_bake_goes_to_the_new_place() {
    let doc = showcase();
    let mut studio = Studio::new(&doc);
    let end = Instant::now() + Duration::from_secs(60);
    while !studio.is_current(&doc) {
        studio.tick(&[], PANE, &doc);
        assert!(Instant::now() < end, "no preview");
        std::thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(studio.light, Light::Draft);
    let level = temp_lit("draft.msl");
    let lit = temp_lit("draft.msl.lit");
    assert!(studio.save_lightmap(&level, &doc).is_none());
    settle_log(&mut studio, &doc);
    assert!(lit.is_file(), "the bake wrote next to the saved level");
}
