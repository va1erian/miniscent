//! The real executable on a real level file with a script beside it.

use miniscent_game::levels::corridor_level;
use miniscent_game::scripts::{self, ScriptWatcher};
use miniscent_game::{GameSession, SessionConfig};
use mscent_math::Vec3;
use mscent_world::{Entity, EntityLabel, Level, format};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// A directory of this test's own, so parallel runs cannot meet.
fn temp_dir(name: &str) -> PathBuf {
    let dir =
        std::env::temp_dir().join(format!("miniscent-scripted-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir
}

/// The corridor level with no goal and a zone named `start_room` around the
/// player's start, so a script is the only way to end it.
fn level() -> Level {
    let mut level = corridor_level();
    level.entities.retain(|e| !matches!(e, Entity::Goal { .. }));
    level.labels.push(EntityLabel {
        entity: level.entities.len() as u32,
        name: Some("start_room".to_string()),
        tags: Vec::new(),
    });
    level.entities.push(Entity::Zone {
        pos: Vec3::new(8.0, 3.0, 2.0),
        half: Vec3::new(2.0, 2.0, 2.0),
    });
    level
}

fn play(dir: &Path, script: Option<&str>) -> Output {
    let path = dir.join("level.msl");
    std::fs::write(&path, format::write(&level())).expect("write level");
    if let Some(script) = script {
        std::fs::write(dir.join("level.st"), script).expect("write script");
    }
    let out = Command::new(env!("CARGO_BIN_EXE_miniscent"))
        .arg("--level")
        .arg(&path)
        .arg("--screenshot")
        .arg(dir.join("shot.png"))
        .args(["--frames", "20", "--res", "64x36", "--no-cache"])
        .output()
        .expect("the game runs");
    let _ = std::fs::remove_dir_all(dir);
    out
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

#[test]
fn the_game_finds_the_script_beside_the_level_and_plays_it() {
    let dir = temp_dir("plays");
    let script = "(zone: 'start_room') onEnter: [ say: 'hello from the script'. win ].";
    let out = play(&dir, Some(script));
    let (stdout, stderr) = (text(&out.stdout), text(&out.stderr));
    assert!(out.status.success(), "{stdout}\n{stderr}");
    assert!(stdout.contains("final phase Won"), "{stdout}\n{stderr}");
    assert!(stderr.contains("hello from the script"), "{stderr}");
}

#[test]
fn a_script_that_does_not_load_stops_the_game_and_says_where() {
    let dir = temp_dir("typo");
    let out = play(
        &dir,
        Some("say: 'fine'.\n(zone: 'start_rom') onEnter: [ win ]."),
    );
    let stderr = text(&out.stderr);
    assert!(!out.status.success());
    assert!(stderr.contains("level.st:2:"), "{stderr}");
    assert!(stderr.contains("did you mean 'start_room'"), "{stderr}");
}

#[test]
fn a_scriptless_level_with_no_goal_is_still_refused() {
    let dir = temp_dir("nogoal");
    let out = play(&dir, None);
    assert!(!out.status.success(), "{}", text(&out.stdout));
}

#[test]
fn the_shared_library_is_available_to_a_level_script() {
    let dir = temp_dir("library");
    // No such mover here, so this reaches `autoDoor:` and fails inside it,
    // which proves the library loaded and the failure names its file.
    let script = "autoDoor: 'nothing' travel: (x: 0 y: 0 z: 1) key: nil. win.";
    let out = play(&dir, Some(script));
    let stderr = text(&out.stderr);
    assert!(stderr.contains("doors.st:"), "{stderr}");
    assert!(stderr.contains("no mover named 'nothing'"), "{stderr}");
}

#[test]
fn saving_a_script_restarts_the_level_on_it_and_a_broken_one_changes_nothing() {
    let dir = temp_dir("reload");
    let level_path = dir.join("level.msl");
    let script = dir.join("level.st");
    let materials = miniscent_game::assets::load_materials(None).expect("textures");
    let cfg = SessionConfig::default();
    let mut session = GameSession::new(&corridor_level(), materials, &cfg).expect("level");
    std::fs::write(&script, "player give: 'first'.").unwrap();
    let files = scripts::for_level(&level_path).unwrap().expect("a script");
    scripts::start(&mut session, &files).unwrap();
    assert!(session.game().has_item("first"));
    let mut watcher = ScriptWatcher::new(&level_path);
    assert_eq!(watcher.reload_if_changed(&mut session), Ok(false));

    std::fs::write(&script, "player give: 'second one'.").unwrap();
    assert_eq!(watcher.reload_if_changed(&mut session), Ok(true));
    assert!(session.game().has_item("second one"));
    assert!(!session.game().has_item("first"), "the level started over");

    std::fs::write(&script, "player give: 'third'. nosuchthing.").unwrap();
    let problem = watcher.reload_if_changed(&mut session).unwrap_err();
    assert!(problem.contains("level.st:1:"), "{problem}");
    assert!(
        session.game().has_item("second one"),
        "the old script runs on"
    );
    let _ = std::fs::remove_dir_all(dir);
}
