//! The shipped sample of level scripting: it is what its source says, and it
//! plays from start to goal.

use std::path::PathBuf;
use std::process::Command;

fn asset(name: &str) -> PathBuf {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets/levels");
    root.join(name)
}

fn game(args: &[&std::ffi::OsStr]) -> String {
    let run = Command::new(env!("CARGO_BIN_EXE_miniscent"))
        .args(args)
        .output()
        .unwrap();
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    assert!(run.status.success(), "{text}");
    text
}

#[test]
fn the_facility_plays_from_start_to_goal() {
    game(&[
        "--level".as_ref(),
        asset("facility.msl").as_os_str(),
        "--test".as_ref(),
        asset("facility.test.st").as_os_str(),
    ]);
}

#[test]
fn the_packed_facility_matches_its_text() {
    let packed = std::env::temp_dir().join(format!("mscent-facility-{}.msl", std::process::id()));
    game(&[
        "--pack-level".as_ref(),
        asset("facility.txt").as_os_str(),
        packed.as_os_str(),
    ]);
    let same = std::fs::read(&packed).unwrap() == std::fs::read(asset("facility.msl")).unwrap();
    let _ = std::fs::remove_file(&packed);
    assert!(same, "pack facility.txt again: see the top of that file");
}

#[test]
fn the_packed_monument_matches_its_text() {
    let packed = std::env::temp_dir().join(format!("mscent-facing-{}.msl", std::process::id()));
    game(&[
        "--pack-level".as_ref(),
        asset("facing.txt").as_os_str(),
        packed.as_os_str(),
    ]);
    let same = std::fs::read(&packed).unwrap() == std::fs::read(asset("facing.msl")).unwrap();
    let _ = std::fs::remove_file(&packed);
    assert!(same, "pack facing.txt again: see the top of that file");
}

#[test]
fn the_monument_has_no_vista_warnings() {
    let text = game(&[
        "--level".as_ref(),
        asset("facing.msl").as_os_str(),
        "--check".as_ref(),
    ]);
    assert!(!text.contains("warning:"), "{text}");
}
