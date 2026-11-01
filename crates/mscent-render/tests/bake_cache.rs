//! The lightmap is baked once per distinct set of inputs: a cache hit shares
//! the same data, a changed light invalidates it, and a damaged cache file is
//! ignored.

use std::path::{Path, PathBuf};

use mscent_math::Vec3;
use mscent_render::{BakeCache, BuildConfig, LevelRenderer, MaterialTable, Source};
use mscent_world::{Entity, Level, showcase_level};

fn materials() -> MaterialTable {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/textures");
    MaterialTable::load_dir(&dir).expect("textures")
}

fn build(level: &Level, cache: &mut BakeCache) -> LevelRenderer {
    LevelRenderer::from_level_cached(level, materials(), &BuildConfig::default(), cache)
        .expect("level builds")
}

fn bytes_of(r: &LevelRenderer) -> Vec<u8> {
    // The atlas and every luxel, through the public store format.
    r.lightmap().to_bytes(0)
}

fn nudge_first_light(level: &mut Level) {
    let light = level
        .entities
        .iter_mut()
        .find(|e| matches!(e, Entity::Light { .. }))
        .expect("showcase has lights");
    if let Entity::Light { pos, .. } = light {
        *pos = *pos + Vec3::new(0.5, 0.0, 0.0);
    }
}

fn temp_file(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("mscent_bake_cache_{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir.join(name)
}

#[test]
fn rebuilding_an_unchanged_level_does_not_bake_again() {
    let level = showcase_level();
    let mut cache = BakeCache::new();
    let first = build(&level, &mut cache);
    assert_eq!(
        (cache.bakes(), cache.last_source()),
        (1, Some(Source::Baked))
    );
    let second = build(&level, &mut cache);
    assert_eq!(
        (cache.bakes(), cache.last_source()),
        (1, Some(Source::Memory))
    );
    assert_eq!(bytes_of(&first), bytes_of(&second), "identical atlas bytes");
    assert!(first.lightmap().atlas() == second.lightmap().atlas());
}

#[test]
fn entities_that_do_not_light_do_not_invalidate() {
    let mut level = showcase_level();
    let mut cache = BakeCache::new();
    build(&level, &mut cache);
    for e in &mut level.entities {
        if let Entity::Enemy { pos, .. } = e {
            *pos = *pos + Vec3::new(1.0, 0.0, 0.0);
        }
    }
    build(&level, &mut cache);
    assert_eq!(cache.bakes(), 1, "moving an enemy must not re-bake");
}

#[test]
fn a_moved_light_invalidates_the_cache() {
    let mut level = showcase_level();
    let mut cache = BakeCache::new();
    let before = build(&level, &mut cache);
    nudge_first_light(&mut level);
    let after = build(&level, &mut cache);
    assert_eq!(
        (cache.bakes(), cache.last_source()),
        (2, Some(Source::Baked))
    );
    assert_ne!(bytes_of(&before), bytes_of(&after));
}

#[test]
fn a_cache_file_serves_a_fresh_cache_and_a_damaged_one_is_ignored() {
    let level = showcase_level();
    let path = temp_file("showcase.msl.lit");
    let _ = std::fs::remove_file(&path);
    let baked = build(&level, &mut BakeCache::with_file(path.clone()));
    assert!(path.is_file(), "the bake wrote its cache file");

    let mut from_file = BakeCache::with_file(path.clone());
    let loaded = build(&level, &mut from_file);
    assert_eq!(
        (from_file.bakes(), from_file.last_source()),
        (0, Some(Source::File))
    );
    assert_eq!(bytes_of(&baked), bytes_of(&loaded));

    // Flip bytes in the middle and cut the end off: both must bake again.
    let good = std::fs::read(&path).expect("read");
    let mut flipped = good.clone();
    flipped[20] ^= 0xff;
    for damaged in [flipped, good[..good.len() / 2].to_vec(), b"junk".to_vec()] {
        std::fs::write(&path, damaged).expect("write");
        let mut cache = BakeCache::with_file(path.clone());
        let r = build(&level, &mut cache);
        assert_eq!(
            (cache.bakes(), cache.last_source()),
            (1, Some(Source::Baked))
        );
        assert_eq!(bytes_of(&baked), bytes_of(&r), "the re-bake is the same");
    }
    let _ = std::fs::remove_file(&path);
}

#[test]
fn renderers_share_one_lightmap_through_the_cache() {
    let level = showcase_level();
    let mut cache = BakeCache::new();
    let a = build(&level, &mut cache);
    let b = build(&level, &mut cache);
    let ptr = |r: &LevelRenderer| std::ptr::from_ref(r.lightmap());
    assert_eq!(ptr(&a), ptr(&b), "one allocation, shared");
}
