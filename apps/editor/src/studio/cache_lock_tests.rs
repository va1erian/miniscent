//! A job can hold the lightmap cache at any moment, to read or write its file.
//! Nothing the UI thread does may wait for that: a Save or an Open behind it
//! would freeze the editor.

use std::sync::mpsc::channel;
use std::time::{Duration, Instant};

use super::*;

const HELD: Duration = Duration::from_millis(1500);

fn showcase() -> Document {
    Document::from_level(&mscent_world::showcase_level())
}

/// Runs `f` while another thread holds the cache lock, and returns how long
/// `f` took.
fn while_locked(studio: &mut Studio, f: impl FnOnce(&mut Studio)) -> Duration {
    let cache = studio.cache();
    let (locked, wait) = channel();
    let t = std::thread::spawn(move || {
        let _held = cache.lock().expect("cache");
        locked.send(()).expect("signal");
        std::thread::sleep(HELD);
    });
    wait.recv().expect("locked");
    let start = Instant::now();
    f(studio);
    let took = start.elapsed();
    t.join().expect("holder");
    took
}

fn temp(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("mscent_cache_lock_{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("dir");
    dir.join(name)
}

#[test]
fn opening_a_level_does_not_wait_for_the_cache() {
    let mut studio = Studio::new(&showcase());
    let level = temp("a.msl");
    let took = while_locked(&mut studio, |s| s.set_cache_level(Some(&level)));
    assert!(took < HELD / 4, "set_cache_level waited {took:?}");
}

#[test]
fn saving_does_not_wait_for_the_cache_and_the_path_still_applies() {
    let doc = showcase();
    let mut studio = Studio::new(&doc);
    let level = temp("b.msl");
    let _ = std::fs::remove_file(temp("b.msl.lit"));
    let mut note = None;
    let took = while_locked(&mut studio, |s| note = s.save_lightmap(&level, &doc));
    assert!(took < HELD / 4, "save_lightmap waited {took:?}");
    assert!(note.is_none(), "nothing was baked yet: {note:?}");
    // The cache is free again: the automatic bake writes next to the level.
    let end = Instant::now() + Duration::from_secs(60);
    while studio.preview(&doc) != Preview::Full {
        let _ = studio.tick(&[], (480, 300), &doc);
        assert!(Instant::now() < end, "never baked");
        std::thread::sleep(Duration::from_millis(5));
    }
    let lit = temp("b.msl.lit");
    assert!(lit.is_file(), "the cache file did not move to {lit:?}");
}
