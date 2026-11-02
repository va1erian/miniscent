//! A new material table reaches the 3D view through the preview path and keeps
//! the lightmap: bounce light uses one configured albedo, and the light key is
//! made from faces, lights and bake settings, none of which name a texture.

use std::time::{Duration, Instant};

use mscent_render::{Material, MaterialId, MaterialTable, Texture};

use super::*;
use crate::build::{self, Light};
use crate::materials::SharedMaterials;

const PANE: (u32, u32) = (480, 300);

fn showcase() -> Document {
    Document::from_level(&mscent_world::showcase_level())
}

/// The stock names, every material one flat colour.
fn painted(color: u32) -> MaterialTable {
    let mut table = MaterialTable::new();
    for name in mscent_world::materials::NAMES {
        table.add(Material {
            name: name.into(),
            gloss: 0,
            texture: Texture::new(128, 128, vec![color; 128 * 128]).expect("a texture"),
        });
    }
    table
}

/// Ticks until the picture is fully lit and the render thread has had time to
/// show it (it draws what changed and then rests), and returns the last frame.
fn settle(studio: &mut Studio, doc: &Document) -> Frame {
    let end = Instant::now() + Duration::from_secs(120);
    let mut last = None;
    let mut rest = None;
    loop {
        last = studio.tick(&[], PANE, doc).frame.or(last);
        if studio.preview(doc) != Preview::Full {
            rest = None;
        } else if rest.get_or_insert_with(Instant::now).elapsed() > Duration::from_millis(500)
            && last.is_some()
        {
            return last.expect("a frame");
        }
        assert!(Instant::now() < end, "never settled");
        std::thread::sleep(Duration::from_millis(3));
    }
}

#[test]
fn a_new_table_changes_the_picture_and_keeps_the_light() {
    let doc = showcase();
    let mut studio = Studio::new(&doc);
    let before = settle(&mut studio, &doc);
    let key = studio.light_key;
    let bakes = studio.cache().lock().expect("cache").bakes();
    assert_eq!(bakes, 1);

    let generation = studio.materials().replace(painted(0xff20_c040));
    studio.tick(&[], PANE, &doc);
    assert_eq!(
        studio.preview(&doc),
        Preview::Updating,
        "the picture is known to be behind"
    );
    let after = settle(&mut studio, &doc);
    assert_eq!(studio.built_materials, generation);

    assert_ne!(before.rgba, after.rgba, "the new colours reached the view");
    let green = |f: &Frame| {
        let (pixels, _) = f.rgba.as_chunks::<4>();
        pixels.iter().filter(|p| p[1] > p[0] && p[1] > p[2]).count()
    };
    assert!(
        green(&after) > green(&before) * 2,
        "{} green pixels, {} before",
        green(&after),
        green(&before)
    );
    assert_eq!(studio.light_key, key, "the light key does not see textures");
    assert_eq!(studio.light, Light::Full, "no draft in between");
    assert_eq!(
        studio.cache().lock().expect("cache").bakes(),
        bakes,
        "nothing was baked again"
    );
}

#[test]
fn a_preview_shares_the_tables_pixels_and_the_cached_lightmap() {
    let doc = showcase();
    let cache = build::SharedCache::default();
    let store = SharedMaterials::default();
    let cfg = SessionConfig::default();
    let finish = |job: Job<Built>| loop {
        if let Some(r) = job.poll() {
            return r.expect("builds");
        }
        std::thread::sleep(Duration::from_millis(5));
    };
    let draft = finish(build::start_preview(&doc, &cache, &store, &cfg).expect("starts"));
    let bake = build::start_bake(&doc, draft.light_key, &cache, &cfg).expect("starts");
    while bake.job.poll().is_none() {
        std::thread::sleep(Duration::from_millis(5));
    }
    let first = finish(build::start_preview(&doc, &cache, &store, &cfg).expect("starts"));
    store.replace(painted(0xff20_c040));
    let second = finish(build::start_preview(&doc, &cache, &store, &cfg).expect("starts"));
    assert_eq!((first.materials, second.materials), (0, 1));
    assert_eq!((first.light, second.light), (Light::Full, Light::Full));
    assert_eq!(first.light_key, second.light_key);
    assert!(
        std::ptr::eq(first.renderer().lightmap(), second.renderer().lightmap()),
        "the same lightmap, not an equal one"
    );
    let table = store.current().1.expect("installed");
    let id = MaterialId(0);
    assert!(
        std::ptr::eq(
            &table.get(id).expect("brick").texture,
            second.renderer().scene().texture_of(id.0),
        ),
        "the renderer draws with the table's pixels, not a copy"
    );
    assert_eq!(cache.lock().expect("cache").bakes(), 1);
}

#[test]
fn a_table_without_the_stock_materials_is_reported_and_the_picture_stays() {
    let doc = showcase();
    let mut studio = Studio::new(&doc);
    settle(&mut studio, &doc);
    studio.materials().replace(MaterialTable::new());
    let end = Instant::now() + Duration::from_secs(30);
    while studio.preview(&doc) != Preview::Failed {
        studio.tick(&[], PANE, &doc);
        assert!(Instant::now() < end, "no failure reported");
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(studio.worker.is_some(), "the last good picture stays up");
    // A good table is a way out, with no edit to the document.
    studio.materials().replace(painted(0xff20_c040));
    let end = Instant::now() + Duration::from_secs(30);
    while studio.tick(&[], PANE, &doc).status.is_none() && studio.error.is_some() {
        assert!(Instant::now() < end, "still failing");
        std::thread::sleep(Duration::from_millis(5));
    }
    settle(&mut studio, &doc);
}
