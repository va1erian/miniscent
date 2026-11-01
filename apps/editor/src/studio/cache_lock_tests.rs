//! The build thread holds the lightmap cache for the whole bake. Nothing the
//! UI thread does may wait for it: a Save or an Open during a bake would
//! freeze the editor for seconds.

use std::sync::mpsc::channel;
use std::time::{Duration, Instant};

use super::*;

const BAKE: Duration = Duration::from_millis(1500);

fn showcase() -> Document {
    Document::from_level(&mscent_world::showcase_level())
}

/// Runs `f` while another thread holds the cache lock like a bake does, and
/// returns how long `f` took.
fn during_a_bake(studio: &mut Studio, f: impl FnOnce(&mut Studio)) -> Duration {
    let cache = studio.cache();
    let (locked, wait) = channel();
    let t = std::thread::spawn(move || {
        let _held = cache.lock().expect("cache");
        locked.send(()).expect("signal");
        std::thread::sleep(BAKE);
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
fn opening_a_level_during_a_bake_does_not_wait_for_it() {
    let mut studio = Studio::new(&showcase());
    let level = temp("a.msl");
    let took = during_a_bake(&mut studio, |s| s.set_cache_level(Some(&level)));
    assert!(took < BAKE / 4, "set_cache_level waited {took:?}");
}

#[test]
fn saving_during_a_bake_does_not_wait_for_it_and_the_path_still_applies() {
    let doc = showcase();
    let mut studio = Studio::new(&doc);
    let level = temp("b.msl");
    let _ = std::fs::remove_file(temp("b.msl.lit"));
    let mut note = None;
    let took = during_a_bake(&mut studio, |s| note = s.save_lightmap(&level, &doc));
    assert!(took < BAKE / 4, "save_lightmap waited {took:?}");
    assert!(note.is_none(), "nothing was built yet: {note:?}");
    // The bake is over: the next build writes next to the level.
    studio.start_build(&doc, false).expect("starts");
    while studio.building() {
        let _ = studio.tick(&[], (480, 300), &doc);
        std::thread::sleep(Duration::from_millis(5));
    }
    let lit = temp("b.msl.lit");
    assert!(lit.is_file(), "the cache file did not move to {lit:?}");
}
