//! Finding a level's script: `foo.st` beside `foo.msl`, plus the shared library
//! in `assets/scripts/lib`. A level with no script file is an ordinary level.

use crate::session::GameSession;
use mscent_script::{MAX_SOURCE, ScriptError, Source};
use std::path::{Path, PathBuf};

/// Script text ready to compile, in load order: libraries, then the level.
pub struct ScriptFiles {
    files: Vec<(String, String)>,
}

/// Reads one script, refusing anything too big before reading it. Scripts ship
/// with levels, so they are untrusted like any other file.
fn read(path: &Path) -> Result<(String, String), String> {
    let shown = path.display();
    let len = std::fs::metadata(path)
        .map_err(|e| format!("cannot read {shown}: {e}"))?
        .len();
    if len > MAX_SOURCE as u64 {
        return Err(format!("{shown} is {len} bytes; the limit is {MAX_SOURCE}"));
    }
    let text = std::fs::read_to_string(path).map_err(|e| format!("cannot read {shown}: {e}"))?;
    let name = path
        .file_name()
        .map_or_else(|| shown.to_string(), |n| n.to_string_lossy().into_owned());
    Ok((name, text))
}

/// The library files in name order, so load order does not depend on the disk.
/// No library directory is not an error.
fn library(dir: &Path) -> Result<Vec<PathBuf>, String> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Ok(Vec::new());
    };
    let mut paths: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "st"))
        .collect();
    paths.sort();
    Ok(paths)
}

/// The library next to the levels' `assets` directory, or the one in the
/// source tree this was built from.
fn library_dir(level: &Path) -> PathBuf {
    let beside = level
        .parent()
        .and_then(Path::parent)
        .map(|assets| assets.join("scripts/lib"));
    match beside {
        Some(dir) if dir.is_dir() => dir,
        _ => Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/scripts/lib"),
    }
}

/// The scripts for a level, or `None` if it has none of its own.
pub fn for_level(level: &Path) -> Result<Option<ScriptFiles>, String> {
    let script = level.with_extension("st");
    if !script.is_file() {
        return Ok(None);
    }
    let mut files = Vec::new();
    for path in library(&library_dir(level))? {
        files.push(read(&path)?);
    }
    files.push(read(&script)?);
    Ok(Some(ScriptFiles { files }))
}

/// Compiles and starts them, failing with every error found. Returns the
/// compiler's warnings.
pub fn start(session: &mut GameSession, scripts: &ScriptFiles) -> Result<Vec<ScriptError>, String> {
    let sources: Vec<Source> = scripts
        .files
        .iter()
        .map(|(name, text)| Source { name, text })
        .collect();
    session.load_script(&sources).map_err(|errors| {
        let lines: Vec<String> = errors.iter().map(ToString::to_string).collect();
        format!("the level's script did not load:\n{}", lines.join("\n"))
    })
}
