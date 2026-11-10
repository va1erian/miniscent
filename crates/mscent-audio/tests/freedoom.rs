//! The sounds that ship in `assets/sounds` are real files this crate reads, and
//! they travel with the license that lets us ship them.

use std::path::{Path, PathBuf};

use mscent_audio::SoundBank;

fn dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/sounds")
}

#[test]
fn every_shipped_wav_parses_and_is_audible() {
    let (bank, skipped) = SoundBank::load_dir(&dir()).unwrap();
    assert!(skipped.is_empty(), "{skipped:?}");
    assert!(bank.len() >= 16, "only {} sounds", bank.len());
    for name in ["dspistol", "dsplpain", "dsbarexp"] {
        let s = bank.get(name).unwrap_or_else(|| panic!("{name} missing"));
        let peak = s.samples.iter().map(|v| v.unsigned_abs()).max().unwrap();
        assert!(peak > 2000, "{name} is near silent (peak {peak})");
        assert!(s.samples.len() > 500, "{name} is too short");
    }
}

#[test]
fn the_license_ships_with_the_sounds() {
    let text = std::fs::read_to_string(dir().join("COPYING-FREEDOOM.txt")).unwrap();
    assert!(text.contains("Contributors to the Freedoom project"));
    assert!(text.contains("Redistributions in binary form must reproduce"));
    let readme = std::fs::read_to_string(dir().join("README.md")).unwrap();
    for f in std::fs::read_dir(dir()).unwrap() {
        let name = f.unwrap().file_name().to_string_lossy().into_owned();
        if name.ends_with(".wav") {
            assert!(readme.contains(&name), "{name} not listed in README.md");
        }
    }
}
