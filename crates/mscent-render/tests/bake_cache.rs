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

/// For a test that counts temporary files: the other tests write some too.
fn own_dir(name: &str) -> PathBuf {
    let dir = temp_file(name);
    std::fs::create_dir_all(&dir).expect("own dir");
    dir
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

#[test]
fn unusable_files_are_ignored_with_a_reason_and_replaced() {
    let level = showcase_level();
    let path = temp_file("reasons.msl.lit");
    let _ = std::fs::remove_file(&path);
    build(&level, &mut BakeCache::with_file(path.clone()));
    let good = std::fs::read(&path).expect("read");

    // The same file for a level whose light moved: stale, not corrupt.
    let mut moved = showcase_level();
    nudge_first_light(&mut moved);
    let mut cache = BakeCache::with_file(path.clone());
    build(&moved, &mut cache);
    let report = cache.last_report().expect("report").clone();
    assert_eq!((report.source, report.written), (Source::Baked, true));
    let note = report.note.as_deref().unwrap_or_default();
    assert!(note.contains("stale"), "{note}");
    assert!(report.describe().contains("cache file ignored"));
    // ...and the file now belongs to the moved level.
    let mut again = BakeCache::with_file(path.clone());
    build(&moved, &mut again);
    assert_eq!(again.last_source(), Some(Source::File));

    let mut old_version = good.clone();
    old_version[4] ^= 0x7f;
    let mut bad_sum = good.clone();
    *bad_sum.last_mut().expect("bytes") ^= 1;
    let cases: [(&str, Vec<u8>, &str); 5] = [
        ("empty", Vec::new(), "truncated"),
        ("junk", b"not a lightmap at all".to_vec(), "not a lightmap"),
        ("version", old_version, "bake version"),
        ("checksum", bad_sum, "damaged"),
        ("cut", good[..good.len() / 3].to_vec(), "damaged"),
    ];
    for (name, bytes, expect) in cases {
        std::fs::write(&path, bytes).expect("write");
        let mut cache = BakeCache::with_file(path.clone());
        build(&level, &mut cache);
        let r = cache.last_report().expect("report");
        let note = r.note.as_deref().unwrap_or_default();
        assert!(
            r.source == Source::Baked && note.contains(expect),
            "{name}: {r:?}"
        );
    }
    // A directory where the file should be is a write error, not a panic.
    let blocked = temp_file("blocked.msl.lit");
    std::fs::create_dir_all(&blocked).expect("dir");
    let mut cache = BakeCache::with_file(blocked);
    build(&level, &mut cache);
    let r = cache.last_report().expect("report");
    assert!(!r.written && r.note.as_deref().is_some_and(|n| n.contains("not written")));
}

#[test]
fn writes_are_atomic_and_rebake_overwrites() {
    let level = showcase_level();
    let path = own_dir("atomic").join("atomic.msl.lit");
    let _ = std::fs::remove_file(&path);
    let mut cache = BakeCache::with_file(path.clone());
    build(&level, &mut cache);
    let leftovers = std::fs::read_dir(path.parent().expect("dir"))
        .expect("list")
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().ends_with(".tmp"))
        .count();
    assert_eq!(leftovers, 0, "temporary files are renamed away");

    cache.force_rebake();
    build(&level, &mut cache);
    assert_eq!(
        (cache.bakes(), cache.last_source()),
        (2, Some(Source::Baked))
    );
    let mut fresh = BakeCache::with_file(path.clone());
    fresh.force_rebake();
    build(&level, &mut fresh);
    assert_eq!(fresh.bakes(), 1, "--rebake ignores a valid file");

    // save_to writes the held bake to a new place, which then serves.
    let copy = temp_file("copy.msl.lit");
    assert_eq!(cache.save_to(copy.clone()), Ok(true));
    let mut from_copy = BakeCache::with_file(copy);
    build(&level, &mut from_copy);
    assert_eq!(from_copy.last_source(), Some(Source::File));
}

#[test]
fn a_warm_start_beats_a_cold_one_on_the_showcase() {
    let level = showcase_level();
    let path = temp_file("timing.msl.lit");
    let _ = std::fs::remove_file(&path);
    let mut cold = BakeCache::with_file(path.clone());
    build(&level, &mut cold);
    let cold_ms = cold.last_report().expect("report").ms;
    // The best of a few: a load descheduled on a busy machine says nothing.
    let mut warm_ms = f64::INFINITY;
    for _ in 0..5 {
        let mut warm = BakeCache::with_file(path.clone());
        build(&level, &mut warm);
        assert_eq!(warm.last_source(), Some(Source::File));
        warm_ms = warm_ms.min(warm.last_report().expect("report").ms);
    }
    let size = std::fs::metadata(&path).expect("file").len();
    println!(
        "showcase lightmap: cold bake {cold_ms:.1} ms, warm load {warm_ms:.1} ms, {size} bytes"
    );
    assert!(warm_ms < cold_ms, "{warm_ms} vs {cold_ms}");
}

/// Two caches of one process (an editor build and a Save, two tests) writing
/// the same file must neither fail nor leave a damaged file.
#[test]
fn concurrent_writers_in_one_process_leave_a_valid_file() {
    let a = showcase_level();
    let mut b = showcase_level();
    nudge_first_light(&mut b);
    let (mut ca, mut cb) = (BakeCache::new(), BakeCache::new());
    build(&a, &mut ca);
    build(&b, &mut cb);
    let dir = own_dir("race");
    let path = dir.join("race.msl.lit");
    let failures = std::sync::atomic::AtomicUsize::new(0);
    std::thread::scope(|s| {
        for cache in [&mut ca, &mut cb] {
            let (path, failures) = (&path, &failures);
            s.spawn(move || {
                for _ in 0..40 {
                    if cache.save_to(path.clone()).is_err() {
                        failures.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    }
                }
            });
        }
    });
    assert_eq!(failures.into_inner(), 0, "a concurrent write failed");
    let mut hit = false;
    for level in [&a, &b] {
        // A miss bakes and overwrites its file, so each level reads its own
        // copy of what the writers left: either may have written last.
        let copy = temp_file("race-copy.msl.lit");
        std::fs::copy(&path, &copy).expect("copied");
        let mut cache = BakeCache::with_file(copy);
        build(level, &mut cache);
        println!("{:?}", cache.last_report());
        hit |= cache.last_source() == Some(Source::File);
    }
    assert!(hit, "the racing writers left an unusable file");
    let leftovers = std::fs::read_dir(&dir)
        .expect("dir")
        .filter_map(Result::ok)
        .filter(|e| e.file_name().to_string_lossy().ends_with(".tmp"))
        .count();
    assert_eq!(leftovers, 0, "temporary files left behind");
}
