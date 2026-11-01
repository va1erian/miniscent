//! The lightmap is baked once and kept: a second headless launch loads the
//! cache file, `--rebake` and a stale file bake again, and nothing in the
//! frame loop (resize, quality change) bakes.

use std::path::{Path, PathBuf};
use std::process::Command;

use miniscent_game::{GameSession, SessionConfig, assets, levels, lightcache};
use mscent_render::{Quality, Source};

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("miniscent_lit_{}_{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir
}

/// Runs the game binary headless for three frames; returns its stdout.
fn launch(cache_dir: &Path, extra: &[&str]) -> String {
    let out = Command::new(env!("CARGO_BIN_EXE_miniscent"))
        .env("MINISCENT_CACHE_DIR", cache_dir)
        .args(["--bench", "--frames", "3", "--res", "160x90"])
        .args(extra)
        .output()
        .expect("runs the game");
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    assert!(
        out.status.success(),
        "{text}{}",
        String::from_utf8_lossy(&out.stderr)
    );
    text
}

fn lightmap_line(out: &str) -> &str {
    out.lines()
        .find(|l| l.starts_with("lightmap "))
        .unwrap_or_else(|| panic!("no lightmap line in:\n{out}"))
}

#[test]
fn the_second_launch_loads_the_cache_instead_of_baking() {
    let dir = temp_dir("launch");
    let file = dir.join("showcase.msl.lit");

    let first = launch(&dir, &[]);
    let l1 = lightmap_line(&first);
    assert!(l1.contains("baked in") && l1.contains("written to"), "{l1}");
    assert!(file.is_file(), "the bake left {}", file.display());

    let second = launch(&dir, &[]);
    let l2 = lightmap_line(&second);
    assert!(l2.contains("loaded from cache"), "{l2}");
    println!("cold: {l1}\nwarm: {l2}");

    let forced = launch(&dir, &["--rebake"]);
    assert!(lightmap_line(&forced).contains("baked in"));

    let off = launch(&dir, &["--no-cache"]);
    assert!(lightmap_line(&off).contains("not cached on disk"));
    let still = launch(&dir, &[]);
    assert!(lightmap_line(&still).contains("loaded from cache"));

    // A damaged file is ignored with a reason, then replaced by a good one.
    let mut bytes = std::fs::read(&file).expect("read");
    let mid = bytes.len() / 2;
    bytes[mid] ^= 0xff;
    std::fs::write(&file, bytes).expect("write");
    let healed = launch(&dir, &[]);
    let l = lightmap_line(&healed);
    assert!(
        l.contains("baked in") && l.contains("cache file ignored"),
        "{l}"
    );
    assert!(lightmap_line(&launch(&dir, &[])).contains("loaded from cache"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn bake_mode_writes_the_cache_next_to_the_level_and_exits() {
    let dir = temp_dir("bake");
    let level = dir.join("corridor.msl");
    std::fs::write(
        &level,
        mscent_world::format::write(&levels::corridor_level()),
    )
    .expect("write");
    let out = launch(&dir, &["--bake", level.to_str().expect("utf8 path")]);
    assert!(
        out.contains("atlas") && lightmap_line(&out).contains("baked in"),
        "{out}"
    );
    assert!(
        !out.contains("frames:"),
        "--bake does not run the game: {out}"
    );
    let lit = dir.join("corridor.msl.lit");
    assert!(lit.is_file());
    let again = launch(&dir, &[level.to_str().expect("utf8 path")]);
    assert!(
        lightmap_line(&again).contains("loaded from cache"),
        "{again}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn nothing_in_the_frame_loop_bakes() {
    let dir = temp_dir("loop");
    let mut other = lightcache::open(Some(&dir.join("x.msl")), "corridor", false, false);
    let materials = || assets::load_materials(None).expect("textures");
    let cfg = SessionConfig::default();
    let mut session =
        GameSession::new_cached(&levels::corridor_level(), materials(), &cfg, &mut other)
            .expect("session");
    assert_eq!(
        (other.bakes(), other.last_source()),
        (1, Some(Source::Baked))
    );
    let before = session.renderer().lightmap().to_bytes(0);
    let ptr = std::ptr::from_ref(session.renderer().lightmap());

    for (w, h, q) in [
        (320, 180, Quality::High),
        (160, 90, Quality::Low),
        (480, 270, Quality::Medium),
    ] {
        session.set_quality(q);
        assert!(session.render(w, h).is_some());
    }
    assert_eq!(other.bakes(), 1);
    assert_eq!(ptr, std::ptr::from_ref(session.renderer().lightmap()));
    assert_eq!(before, session.renderer().lightmap().to_bytes(0));
    let _ = std::fs::remove_dir_all(&dir);
}
