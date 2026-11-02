//! The agent commands, through the real executable, on real files: the loop an
//! agent would run, with no `cargo build` in it.

use miniscent_game::scripts::ScriptWatcher;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// A directory for one test, removed when it ends.
struct Temp(PathBuf);

impl Temp {
    fn new(name: &str) -> Self {
        let dir =
            std::env::temp_dir().join(format!("miniscent-agent-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");
        Self(dir)
    }

    fn path(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }

    fn write(&self, name: &str, text: &str) -> PathBuf {
        let path = self.path(name);
        std::fs::write(&path, text).expect("write");
        path
    }
}

impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn game(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_miniscent"))
        .args(args)
        .output()
        .expect("the game runs")
}

fn s(p: &Path) -> &str {
    p.to_str().expect("a plain path")
}

fn out(o: &Output) -> String {
    format!(
        "{}\n{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}

const LEVEL: &str = "miniscent-level 1
box subtract 0 1  -20 -20 -20  20 20 20
box add 1 2  14 -3 -3  15 3 3
player 0 0 0 0
marker 4 0 0 0  name vault_switch_1
marker 7 0 0 0  name vault_switch_2
marker 10 0 0 0  name vault_switch_3
marker 0 15 0 0  name spare
mover vault_door brushes 1
";

const SCRIPT: &str = "
sw := defineEnemy: 'switch' base: 0.
sw hp: 25.
sw speed: 0.
sw damage: 0.
vault := mover: 'vault_door'.
vault pose: 'open' move: (x: 0 y: 0 z: 18).
(markers: 'vault_switch') do: [:m | spawn: 'switch' at: m tag: 'vault_switch'].
(tagged: 'vault_switch') onAllDestroyed: [
    vault moveTo: 'open' in: 0.5.
    say: 'Vault unlocked \"now\"'.
].
";

const TEST: &str = "
vault := mover: 'vault_door'.
assert: vault poseNow = 'closed' because: 'the vault starts closed'.
(tagged: 'vault_switch') members do: [:s |
    teleport: s distance: 3.
    aimAt: s.
    fireFor: 0.7.
].
wait: 1.
assert: vault poseNow = 'open' because: 'the vault opens once every switch is gone'.
assert: (tagged: 'vault_switch') alive = 0 because: 'every switch is gone'.
";

/// The level packed from text, with its script beside it.
fn make_level(dir: &Temp) -> PathBuf {
    let text = dir.write("level.txt", LEVEL);
    let level = dir.path("level.msl");
    let packed = game(&["--pack-level", s(&text), s(&level)]);
    assert!(packed.status.success(), "{}", out(&packed));
    dir.write("level.st", SCRIPT);
    level
}

#[test]
fn an_agent_can_build_and_test_a_switch_door_from_nothing() {
    let dir = Temp::new("loop");
    let level = make_level(&dir);
    let check = game(&["--level", s(&level), "--check"]);
    assert!(check.status.success(), "{}", out(&check));
    let test = dir.write("level.test.st", TEST);
    let trace = dir.path("trace.jsonl");
    let run = game(&[
        "--level",
        s(&level),
        "--test",
        s(&test),
        "--trace",
        s(&trace),
    ]);
    let text = out(&run);
    assert!(run.status.success(), "{text}");
    assert!(
        text.contains("3 passed, 0 failed, 0 script error(s)"),
        "{text}"
    );
    let log = std::fs::read_to_string(&trace).expect("a trace");
    for wanted in [
        r#""ev":"spawned""#,
        r#""ev":"destroyed""#,
        r#""ev":"move_to","mover":"vault_door","pose":"open""#,
        r#""ev":"arrived","mover":"vault_door","pose":"open""#,
        r#""say":"Vault unlocked \"now\"""#,
        r#""check":true,"because":"the vault opens once every switch is gone""#,
    ] {
        assert!(log.contains(wanted), "{wanted} not in\n{log}");
    }
    for line in log.lines() {
        assert!(line.starts_with("{\"t\":") && line.ends_with('}'), "{line}");
    }
}

#[test]
fn the_same_test_writes_the_same_trace_byte_for_byte() {
    let dir = Temp::new("replay");
    let level = make_level(&dir);
    let test = dir.write("level.test.st", TEST);
    let (a, b) = (dir.path("a.jsonl"), dir.path("b.jsonl"));
    for trace in [&a, &b] {
        let run = game(&[
            "--level",
            s(&level),
            "--test",
            s(&test),
            "--trace",
            s(trace),
        ]);
        assert!(run.status.success(), "{}", out(&run));
    }
    let bytes = std::fs::read(&a).unwrap();
    assert!(!bytes.is_empty());
    assert_eq!(bytes, std::fs::read(&b).unwrap());
}

#[test]
fn a_failing_test_fails_the_process_and_says_why() {
    let dir = Temp::new("fails");
    let level = make_level(&dir);
    let test = dir.write(
        "t.st",
        "assert: false because: 'this is the reason'.\nassert: true because: 'fine'.",
    );
    let run = game(&["--level", s(&level), "--test", s(&test)]);
    let text = out(&run);
    assert!(!run.status.success(), "{text}");
    assert!(text.contains("FAILED: this is the reason"), "{text}");
    assert!(text.contains("1 passed, 1 failed"), "{text}");
}

#[test]
fn a_script_error_during_a_test_fails_it_with_file_and_line() {
    let dir = Temp::new("error");
    let level = make_level(&dir);
    let test = dir.write("t.st", "wait: 0.1.\n1 / 0.");
    let run = game(&["--level", s(&level), "--test", s(&test)]);
    let text = out(&run);
    assert!(!run.status.success(), "{text}");
    assert!(text.contains("ERROR: t.st:2:"), "{text}");
}

#[test]
fn test_functions_are_not_available_outside_a_test() {
    let dir = Temp::new("notest");
    let level = make_level(&dir);
    dir.write("level.st", "assert: true because: 'x'.");
    let check = game(&["--level", s(&level), "--check"]);
    let text = out(&check);
    assert!(!check.status.success(), "{text}");
    assert!(text.contains("level.st:1:"), "{text}");
}

#[test]
fn a_test_can_see_how_the_level_ended() {
    let dir = Temp::new("ended");
    let level = make_level(&dir);
    let test = dir.write(
        "t.st",
        "win. wait: 0.1.\nassert: phase = 'won' because: 'the script won'.",
    );
    let run = game(&["--level", s(&level), "--test", s(&test)]);
    assert!(run.status.success(), "{}", out(&run));
}

#[test]
fn a_shot_needs_a_plain_name_and_writes_a_png_where_it_was_told() {
    let dir = Temp::new("shot");
    let level = make_level(&dir);
    let shots = dir.path("pictures");
    let test = dir.write("t.st", "shot: 'one.png'.\nshot: '../escape.png'.");
    let run = game(&[
        "--level",
        s(&level),
        "--test",
        s(&test),
        "--shots-dir",
        s(&shots),
    ]);
    let text = out(&run);
    assert!(!run.status.success(), "the bad name is an error: {text}");
    assert!(text.contains("plain file name"), "{text}");
    let png = std::fs::read(shots.join("one.png")).expect("the good shot");
    assert_eq!(&png[1..4], b"PNG");
    assert!(!dir.path("escape.png").exists());
}

#[test]
fn a_door_that_moved_shows_in_the_next_shot() {
    let dir = Temp::new("doorshot");
    let level = make_level(&dir);
    let shots = dir.path("pictures");
    let test = dir.write(
        "t.st",
        "flyTo: (x: 0 y: 0 z: 0).
        aimAt: (mover: 'vault_door').
        wait: 0.05.
        shot: 'shut.png'.
        (mover: 'vault_door') moveTo: 'open' in: 0.1.
        wait: 0.2.
        shot: 'open.png'.",
    );
    let run = game(&[
        "--level",
        s(&level),
        "--test",
        s(&test),
        "--shots-dir",
        s(&shots),
    ]);
    assert!(run.status.success(), "{}", out(&run));
    let (shut, open) = (
        std::fs::read(shots.join("shut.png")).unwrap(),
        std::fs::read(shots.join("open.png")).unwrap(),
    );
    assert_ne!(shut, open, "the door must show");
}

#[test]
fn check_lists_names_warns_about_unused_ones_and_fails_on_errors() {
    let dir = Temp::new("check");
    let level = make_level(&dir);
    let ok = game(&["--level", s(&level), "--check"]);
    let text = out(&ok);
    assert!(text.contains("mover   vault_door"), "{text}");
    assert!(
        text.contains("marker  spare  (no script names it)"),
        "{text}"
    );
    assert!(
        text.contains("1 name(s) in the level no script mentions"),
        "{text}"
    );
    dir.write("level.st", "timerRuning := true.\n");
    let warned = game(&["--level", s(&level), "--check"]);
    let text = out(&warned);
    assert!(warned.status.success(), "{text}");
    assert!(text.contains("warning: level.st:1:"), "{text}");
    dir.write("level.st", "(mover: 'vault_dor') moveTo: 'x' in: 1.");
    let bad = game(&["--level", s(&level), "--check"]);
    let text = out(&bad);
    assert!(!bad.status.success(), "{text}");
    assert!(text.contains("did you mean 'vault_door'"), "{text}");
    dir.write("level.st", "(mover: 'vault_door') moveTo: 'x' in: 1.");
    let runtime = game(&["--level", s(&level), "--check"]);
    assert!(!runtime.status.success(), "{}", out(&runtime));
}

#[test]
fn a_level_dumped_and_packed_again_is_the_same_file() {
    let dir = Temp::new("dump");
    let level = make_level(&dir);
    let dump = game(&["--dump-level", s(&level)]);
    assert!(dump.status.success(), "{}", out(&dump));
    let text = dir.write("back.txt", &String::from_utf8_lossy(&dump.stdout));
    let back = dir.path("back.msl");
    assert!(game(&["--pack-level", s(&text), s(&back)]).status.success());
    assert_eq!(
        std::fs::read(&level).unwrap(),
        std::fs::read(&back).unwrap()
    );
}

#[test]
fn a_bad_level_text_names_its_line_and_writes_nothing() {
    let dir = Temp::new("badtext");
    let text = dir.write("bad.txt", "miniscent-level 1\nfrog 1 2 3\n");
    let target = dir.path("bad.msl");
    let run = game(&["--pack-level", s(&text), s(&target)]);
    let shown = out(&run);
    assert!(!run.status.success(), "{shown}");
    assert!(shown.contains("line 2"), "{shown}");
    assert!(!target.exists());
}

#[test]
fn script_api_lists_what_the_table_registers() {
    let run = game(&["--script-api"]);
    let text = out(&run);
    assert!(run.status.success());
    for wanted in [
        "moveTo:in:",
        "spawn:at:tag:",
        "assert:because:",
        "timesRepeat:",
        "whileTrue:",
    ] {
        assert!(text.contains(wanted), "{wanted}");
    }
    for (name, _) in mscent_game::natives() {
        assert!(text.contains(name), "{name}");
    }
}

#[test]
fn commands_that_need_a_level_say_so() {
    for args in [&["--check"][..], &["--test", "x.st"]] {
        let run = game(args);
        assert!(!run.status.success());
        assert!(out(&run).contains("needs --level"), "{}", out(&run));
    }
}

#[test]
fn the_watcher_notices_changes_to_the_script_and_not_otherwise() {
    let dir = Temp::new("watch");
    let level = dir.path("level.msl");
    dir.write("level.st", "x := 1.");
    let mut watcher = ScriptWatcher::new(&level);
    assert!(!watcher.changed(), "nothing yet");
    dir.write("level.st", "x := 1. y := 2.");
    assert!(watcher.changed());
    assert!(!watcher.changed(), "once");
    std::fs::remove_file(dir.path("level.st")).unwrap();
    assert!(watcher.changed(), "a deleted script is a change");
}
