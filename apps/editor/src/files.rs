//! Reading and writing level source (`.msl`: brushes, entities, materials)
//! with the world crate's versioned, bounds-checked format, plus the
//! remembered "recent" path. Every failure is a message for the status bar.

use std::path::{Path, PathBuf};

use mscent_world::format::{self, MAX_FILE_BYTES};

use crate::errors;
use crate::model::Document;

/// File extension of level files.
pub const EXT: &str = "msl";

/// Writes `doc` to `path` (through a temporary file, so a failed write
/// leaves an existing level intact). Returns the status message.
pub fn save(doc: &Document, path: &Path) -> Result<String, String> {
    let level = doc
        .to_level()
        .map_err(|e| format!("Cannot save: a brush is invalid: {}", errors::brush(e)))?;
    // Flipping a brush can bring two names back together; a file with them
    // would not load again, so the level on disk stays as it was.
    level
        .check_labels()
        .map_err(|e| format!("Cannot save: {}", errors::label(e)))?;
    let bytes = format::write(&level);
    // A file the loader refuses is worse than no save: the old one stays.
    format::read(&bytes).map_err(|e| format!("Cannot save: {}", errors::format(e)))?;
    let tmp = path.with_extension("msl.tmp");
    std::fs::write(&tmp, &bytes).map_err(|e| format!("cannot write {}: {e}", tmp.display()))?;
    if let Err(e) = std::fs::rename(&tmp, path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(format!("cannot write {}: {e}", path.display()));
    }
    Ok(format!(
        "Saved {} ({} brushes, {} entities)",
        path.display(),
        doc.brushes().len(),
        doc.entities().len()
    ))
}

/// Reads a level file. A file that is too large, truncated, corrupt or from
/// a newer version gives an error, never a panic.
pub fn load(path: &Path) -> Result<Document, String> {
    let meta =
        std::fs::metadata(path).map_err(|e| format!("cannot open {}: {e}", path.display()))?;
    if !meta.is_file() {
        return Err(format!("{} is not a file", path.display()));
    }
    if meta.len() > MAX_FILE_BYTES as u64 {
        return Err(format!("{} is too large to be a level", path.display()));
    }
    let data = std::fs::read(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    let level = format::read(&data)
        .map_err(|e| format!("Cannot open {}: {}", path.display(), errors::format(e)))?;
    let mut doc = Document::from_level(&level);
    doc.set_level_path(Some(path.to_path_buf()));
    Ok(doc)
}

/// The editor's own place in the user's settings.
fn config_dir() -> Option<PathBuf> {
    let base = std::env::var_os("APPDATA")
        .or_else(|| std::env::var_os("XDG_CONFIG_HOME"))
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(base.join("miniscent"))
}

/// Where the last used path is remembered.
fn recent_file() -> Option<PathBuf> {
    Some(config_dir()?.join("editor-recent.txt"))
}

/// Where copies of the samples go to be played with.
pub fn samples_base() -> PathBuf {
    config_dir().unwrap_or_else(|| std::env::temp_dir().join("miniscent"))
}

/// The last level opened or saved, if it still exists.
pub fn recent() -> Option<PathBuf> {
    let text = std::fs::read_to_string(recent_file()?).ok()?;
    let path = PathBuf::from(text.lines().next()?.trim());
    path.is_file().then_some(path)
}

/// Remembers `path` for the next File dialog; failures are ignored (the
/// shortcut is a convenience, not data).
pub fn remember(path: &Path) {
    let Some(file) = recent_file() else { return };
    if let Some(dir) = file.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(file, path.display().to_string());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("mscent-editor-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        dir.join(name)
    }

    #[test]
    fn showcase_round_trips_through_a_file() {
        let doc = Document::from_level(&mscent_world::showcase_level());
        let path = temp("showcase.msl");
        let msg = save(&doc, &path).expect("save");
        assert!(msg.contains("Saved"), "{msg}");
        let back = load(&path).expect("load");
        assert_eq!(back.brushes().len(), doc.brushes().len());
        assert_eq!(back.groups(), doc.groups());
        assert_eq!(back.entities().len(), doc.entities().len());
        for (a, b) in doc.brushes().iter().zip(back.brushes()) {
            assert_eq!(
                (a.shape.clone(), a.op, a.material),
                (b.shape.clone(), b.op, b.material)
            );
        }
        for (a, b) in doc.entities().iter().zip(back.entities()) {
            assert_eq!(a.entity, b.entity);
        }
        assert!(
            !path.with_extension("msl.tmp").exists(),
            "no temp file left"
        );
    }

    #[test]
    fn hostile_files_are_errors_not_panics() {
        let good = format::write(&mscent_world::showcase_level());
        let cases: Vec<(&str, Vec<u8>)> = vec![
            ("empty", Vec::new()),
            ("junk", b"definitely not a level".to_vec()),
            ("truncated", good[..good.len() / 2].to_vec()),
            ("header only", good[..12].to_vec()),
            ("trailing", [good.clone(), vec![1, 2, 3]].concat()),
        ];
        for (name, data) in cases {
            let path = temp(&format!("bad-{}.msl", name.replace(' ', "-")));
            std::fs::write(&path, data).expect("write");
            let err = load(&path).expect_err(name);
            assert!(err.starts_with("Cannot open"), "{name}: {err}");
        }
        // Bit flips anywhere must give an error or a (valid) level.
        let path = temp("flip.msl");
        for i in (0..good.len()).step_by(37) {
            let mut bad = good.clone();
            bad[i] ^= 0xff;
            std::fs::write(&path, &bad).expect("write");
            let _ = load(&path);
        }
    }

    #[test]
    fn missing_paths_and_directories_are_errors() {
        assert!(load(&temp("nope.msl")).unwrap_err().contains("cannot open"));
        let dir = temp("a-directory");
        std::fs::create_dir_all(&dir).expect("dir");
        assert!(load(&dir).unwrap_err().contains("not a file"));
        let doc = Document::new();
        assert!(
            save(&doc, &dir).is_err(),
            "writing over a directory fails cleanly"
        );
    }
}
