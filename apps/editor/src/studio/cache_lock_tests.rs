//! A job can hold the lightmap cache at any moment, to read or write its file.
//! Nothing the UI thread does may wait for that: a Save or an Open behind it
//! would freeze the editor. No clock is involved: the lock stays held until the
//! UI thread is done, so a call that waited for it could only get through once
//! the holder gave up, and that is the failure.

use std::sync::mpsc::{RecvTimeoutError, channel};
use std::time::{Duration, Instant};

use super::preview_tests::{bakes, settle, settled, tick_until, with_box};
use super::*;

/// How long the holder lets go of the lock for: far beyond any tick, however
/// busy the machine, so only a thread that really waits gets to see it end.
const PATIENCE: Duration = Duration::from_secs(30);
const PANE: (u32, u32) = (480, 300);
/// Ticks to run while a job is held up.
const TICKS: u32 = 200;

fn showcase() -> Document {
    Document::from_level(&mscent_world::showcase_level())
}

/// Runs `f` while another thread holds the cache lock, which it lets go of only
/// once `f` is done. Fails when `f` got there by waiting for it.
#[track_caller]
fn while_locked(studio: &mut Studio, f: impl FnOnce(&mut Studio)) {
    let cache = studio.cache();
    let (locked, wait) = channel();
    let (done, until) = channel::<()>();
    let t = std::thread::spawn(move || {
        let _held = cache.lock().expect("cache");
        locked.send(()).expect("signal");
        matches!(until.recv_timeout(PATIENCE), Err(RecvTimeoutError::Timeout))
    });
    wait.recv().expect("locked");
    f(studio);
    // A holder that gave up has hung up already; its answer says so below.
    let _ = done.send(());
    let gave_up = t.join().expect("holder");
    assert!(!gave_up, "the UI thread waited for the cache lock");
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
    while_locked(&mut studio, |s| s.set_cache_level(Some(&level)));
}

#[test]
fn saving_does_not_wait_for_the_cache_and_the_path_still_applies() {
    let doc = showcase();
    let mut studio = Studio::new(&doc);
    let level = temp("b.msl");
    let _ = std::fs::remove_file(temp("b.msl.lit"));
    let mut note = None;
    while_locked(&mut studio, |s| note = s.save_lightmap(&level, &doc));
    assert!(note.is_none(), "nothing was baked yet: {note:?}");
    // The cache is free again: the automatic bake writes next to the level.
    let end = Instant::now() + Duration::from_secs(60);
    while studio.preview(&doc) != Preview::Full {
        let _ = studio.tick(&[], PANE, &doc);
        assert!(Instant::now() < end, "never baked");
        std::thread::sleep(Duration::from_millis(5));
    }
    let lit = temp("b.msl.lit");
    assert!(lit.is_file(), "the cache file did not move to {lit:?}");
}

/// A preview looks its lightmap up in the cache, so with the lock held it
/// cannot finish: the ticks that go through meanwhile did not wait for it.
#[test]
fn ticks_go_on_while_a_preview_cannot_finish() {
    let (mut studio, doc) = settled();
    let edited = with_box(&doc, 40.0);
    while_locked(&mut studio, |s| {
        for _ in 0..TICKS {
            s.tick(&[], PANE, &edited);
            assert_eq!(s.preview(&edited), Preview::Updating);
        }
        assert!(s.preview_job.is_some(), "the job is still out");
    });
    settle(&mut studio, &edited);
    assert_eq!(bakes(&studio), 2);
}

/// A bake stores its lightmap in the cache and the preview that fetches it
/// reads it from there, so with the lock held the full light cannot arrive,
/// however far the bake had got when the lock was taken.
#[test]
fn ticks_go_on_while_a_bake_cannot_finish() {
    let (mut studio, doc) = settled();
    let edited = with_box(&doc, 40.0);
    tick_until(&mut studio, &edited, |s| s.is_current(&edited));
    let job_out = |s: &Studio| s.bake_job.is_some() || s.preview_job.is_some();
    while_locked(&mut studio, |s| {
        tick_until(s, &edited, job_out);
        for _ in 0..TICKS {
            s.tick(&[], PANE, &edited);
            assert_eq!(s.preview(&edited), Preview::Baking);
        }
        assert!(job_out(s), "the job is still out");
    });
    settle(&mut studio, &edited);
    assert_eq!(bakes(&studio), 2);
}
