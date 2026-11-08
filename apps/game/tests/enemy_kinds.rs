//! A level's own enemy kinds, end to end through the real executable (M19.2):
//! a level written by hand with two kinds, packed, played by a test script.

use std::path::PathBuf;
use std::process::Command;

const MANTIS: &str = include_str!("../../../assets/enemies/mantis.msb");

/// A `kind` block wearing the mantis, with `hp`.
fn kind(name: &str, hp: u32) -> String {
    let mut out = format!(
        "kind {name} hp={hp} radius=0.75 speed=0 sight=0 attack=0 damage=0 cooldown=1 lose=1\n"
    );
    for line in MANTIS.lines() {
        out += &format!("| {line}\n");
    }
    out + "end\n"
}

fn level() -> String {
    format!(
        "miniscent-level 1
{}{}box subtract 0 1  -20 -20 -20  20 20 20
player 0 0 0 0
enemy 10 -8 0  0  tag weak
enemy 10 8 0  1  tag tough
",
        kind("weakling", 40),
        kind("tank", 100_000),
    )
}

const TEST: &str = "
(tagged: 'weak') members do: [:e | teleport: e distance: 8. aimAt: e. fireFor: 1.5].
(tagged: 'tough') members do: [:e | teleport: e distance: 8. aimAt: e. fireFor: 1.5].
wait: 0.2.
assert: (tagged: 'weak') alive = 0 because: 'a kind of 40 hp dies to two shots'.
assert: (tagged: 'tough') alive = 1 because: 'a kind of 100000 hp outlasts them'.
";

fn game(args: &[&str]) -> String {
    let out = Command::new(env!("CARGO_BIN_EXE_miniscent"))
        .args(args)
        .output()
        .expect("the game runs");
    let text = format!(
        "{}\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(out.status.success(), "{args:?}\n{text}");
    text
}

#[test]
fn a_hand_written_kind_plays_by_its_own_stats() {
    let dir: PathBuf = std::env::temp_dir().join(format!("miniscent-kinds-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let file = |name: &str| dir.join(name).to_str().unwrap().to_string();
    std::fs::write(file("level.txt"), level()).unwrap();
    std::fs::write(file("level.st"), "say: 'ready'.\n").unwrap();
    std::fs::write(file("t.st"), TEST).unwrap();
    game(&["--pack-level", &file("level.txt"), &file("level.msl")]);
    let text = game(&["--level", &file("level.msl"), "--test", &file("t.st")]);
    let _ = std::fs::remove_dir_all(&dir);
    assert!(text.contains("2 passed, 0 failed"), "{text}");
}
