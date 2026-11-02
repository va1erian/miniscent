//! Sample levels with their scripts, inside the editor so they work wherever it
//! runs. Opening one writes a copy to play with beside a copy of the library, so
//! the shipped ones stay as they are and the game finds what they call.

use std::path::{Path, PathBuf};

use mscent_world::{format, level_text};

/// A sample level.
#[derive(Clone, Copy, Debug)]
pub struct Sample {
    pub name: &'static str,
    pub title: &'static str,
    level: &'static str,
    script: &'static str,
    /// Its test, run by the test below in the real game.
    #[cfg_attr(not(test), allow(dead_code))]
    test: &'static str,
}

macro_rules! sample {
    ($name:literal, $title:literal) => {
        Sample {
            name: $name,
            title: $title,
            level: include_str!(concat!("../../../../assets/samples/", $name, ".txt")),
            script: include_str!(concat!("../../../../assets/samples/", $name, ".st")),
            test: include_str!(concat!("../../../../assets/samples/", $name, ".test.st")),
        }
    };
}

pub const SAMPLES: [Sample; 3] = [
    sample!("first_door", "First door: a door and a way out"),
    sample!(
        "keys_and_switches",
        "Keys and switches: locks, a terminal, a pickup"
    ),
    sample!(
        "lift_and_wave",
        "Lift and wave: moving floors, enemies in sequence"
    ),
];

/// The library, so a sample's copy has what it calls.
const LIBRARY: [(&str, &str); 2] = [
    (
        "doors.st",
        include_str!("../../../../assets/scripts/lib/doors.st"),
    ),
    (
        "kit.st",
        include_str!("../../../../assets/scripts/lib/kit.st"),
    ),
];

fn write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    std::fs::write(path, bytes).map_err(|e| format!("cannot write {}: {e}", path.display()))
}

fn make_dir(dir: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))
}

impl Sample {
    /// Writes a fresh copy of the sample and the library under `base`, as a level
    /// beside its script and two directories from the library, which is where
    /// the game looks. Returns the level's path. Opening it again starts over.
    pub fn install(&self, base: &Path) -> Result<PathBuf, String> {
        let level = level_text::from_text(self.level)
            .map_err(|e| format!("the sample {} is broken: {e}", self.name))?;
        let (samples, lib) = (base.join("samples"), base.join("scripts").join("lib"));
        make_dir(&samples)?;
        make_dir(&lib)?;
        for (name, text) in LIBRARY {
            write(&lib.join(name), text.as_bytes())?;
        }
        let path = samples.join(self.name).with_extension(crate::files::EXT);
        write(&path, &format::write(&level))?;
        write(&path.with_extension("st"), self.script.as_bytes())?;
        Ok(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use miniscent_game::agent::{TestOutput, check, run_test};

    fn base(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("mscent-sample-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn every_sample_loads_and_passes_its_test_in_the_real_game() {
        for sample in &SAMPLES {
            let dir = base(sample.name);
            let level = sample.install(&dir).expect("installs");
            let report = check(&level).unwrap_or_else(|e| panic!("{}: {e}", sample.name));
            assert!(report.contains(" 0 warning"), "{}: {report}", sample.name);
            let test = dir.join("test.st");
            std::fs::write(&test, sample.test).unwrap();
            let out = TestOutput {
                trace: None,
                shots_dir: dir.clone(),
                assets: None,
            };
            let result = run_test(&level, &test, &out);
            let summary = result.unwrap_or_else(|e| panic!("{}: {e}", sample.name));
            assert!(summary.contains(" 0 failed"), "{}: {summary}", sample.name);
            assert!(
                summary.contains(" 0 script error"),
                "{}: {summary}",
                sample.name
            );
            let _ = std::fs::remove_dir_all(dir);
        }
    }

    #[test]
    fn the_library_copied_is_the_library_in_the_assets() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/scripts/lib");
        for (name, text) in LIBRARY {
            let on_disk = std::fs::read_to_string(dir.join(name)).expect("reads");
            assert_eq!(on_disk, text, "{name}");
        }
        let names = std::fs::read_dir(&dir).unwrap().flatten();
        let scripts = names.filter(|f| f.path().extension().is_some_and(|x| x == "st"));
        assert_eq!(
            scripts.count(),
            LIBRARY.len(),
            "a library file is not copied"
        );
    }

    #[test]
    fn opening_a_sample_again_starts_it_over() {
        let dir = base("again");
        let sample = &SAMPLES[0];
        let level = sample.install(&dir).unwrap();
        std::fs::write(level.with_extension("st"), "edited").unwrap();
        sample.install(&dir).unwrap();
        let script = std::fs::read_to_string(level.with_extension("st")).unwrap();
        assert_eq!(script, sample.script);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_base_that_cannot_be_written_is_an_error() {
        let dir = base("file");
        std::fs::create_dir_all(dir.parent().unwrap()).unwrap();
        std::fs::write(&dir, b"a file, not a directory").unwrap();
        assert!(SAMPLES[0].install(&dir).unwrap_err().contains("cannot"));
        let _ = std::fs::remove_file(dir);
    }
}
