//! `--check` lists the vista warnings of a level, through the real executable.

use std::path::PathBuf;
use std::process::{Command, Output};

fn game(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_miniscent"))
        .args(args)
        .output()
        .expect("the game runs")
}

fn text(o: &Output) -> String {
    format!(
        "{}
{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}

/// A level packed from text in a directory of its own.
fn packed(name: &str, source: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("miniscent-vista-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp dir");
    let (txt, msl) = (dir.join("level.txt"), dir.join("level.msl"));
    std::fs::write(&txt, source).expect("write");
    let args = ["--pack-level", txt.to_str().unwrap(), msl.to_str().unwrap()];
    let done = game(&args);
    assert!(done.status.success(), "{}", text(&done));
    msl
}

#[test]
fn check_warns_about_a_view_wall_with_no_layer_but_still_passes() {
    // The room's walls are the viewport material of slot 3, and nothing feeds it.
    let level = packed(
        "no-layer",
        "miniscent-level 1
box subtract 0 65523  -8 -8 -8  8 8 8
player 0 0 0 0
goal 4 4 0 1
",
    );
    let check = game(&["--level", level.to_str().unwrap(), "--check"]);
    let out = text(&check);
    assert!(check.status.success(), "{out}");
    assert!(
        out.contains("warning: a wall shows view 3 but no vista layer has slot 3"),
        "{out}"
    );
}

#[test]
fn check_is_quiet_about_a_fed_slot() {
    let level = packed(
        "fed",
        "miniscent-level 1
box subtract 0 65520  -8 -8 -8  8 8 8
box subtract 1 10  1000 0 0  1064 64 64
player 0 0 0 0
goal 4 4 0 1
vista 1032 32 32  0 0 0  0 0 0.0625  0 0 0.1  0 0  0 1e5  0
",
    );
    let check = game(&["--level", level.to_str().unwrap(), "--check"]);
    let out = text(&check);
    assert!(check.status.success(), "{out}");
    assert!(!out.contains("vista"), "{out}");
}
