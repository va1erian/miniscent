//! A folder of `.wav` files, loaded by name.

use std::collections::HashMap;
use std::path::Path;

use crate::wav::{self, Sound};

/// Largest file read. Freedoom's longest effect is 155 KB.
const MAX_FILE_BYTES: u64 = 4 << 20;

#[derive(Clone, Debug, Default)]
pub struct SoundBank {
    sounds: HashMap<String, Sound>,
}

impl SoundBank {
    /// Reads every `.wav` in `dir`, keyed by file stem. A file that cannot be
    /// read or parsed is skipped and listed in the second value: a bad sound
    /// must not stop the game, but it must not vanish silently either.
    pub fn load_dir(dir: &Path) -> std::io::Result<(Self, Vec<String>)> {
        let mut bank = Self::default();
        let mut skipped = Vec::new();
        for entry in std::fs::read_dir(dir)? {
            let path = entry?.path();
            let is_wav = path
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("wav"));
            let stem = path.file_stem().and_then(|s| s.to_str());
            let (true, true, Some(stem)) = (is_wav, path.is_file(), stem) else {
                continue;
            };
            match read_sound(&path) {
                Ok(sound) => {
                    bank.sounds.insert(stem.to_owned(), sound);
                }
                Err(why) => skipped.push(format!("{}: {why}", path.display())),
            }
        }
        skipped.sort();
        Ok((bank, skipped))
    }

    pub fn get(&self, name: &str) -> Option<&Sound> {
        self.sounds.get(name)
    }

    pub fn len(&self) -> usize {
        self.sounds.len()
    }

    pub fn is_empty(&self) -> bool {
        self.sounds.is_empty()
    }
}

fn read_sound(path: &Path) -> Result<Sound, String> {
    let meta = std::fs::metadata(path).map_err(|e| e.to_string())?;
    if meta.len() > MAX_FILE_BYTES {
        return Err("file is too large".into());
    }
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    wav::parse(&bytes).map_err(|e| e.to_string())
}
