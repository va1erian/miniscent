//! Finding a level's script: `foo.st` beside `foo.msl`, plus the shared library
//! in `assets/scripts/lib`. A level with no script file is an ordinary level.
//! [`ScriptWatcher`] reloads them while the game runs.

use crate::session::GameSession;
pub use mscent_script::MAX_SOURCE;
use mscent_script::{ScriptError, Source};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// Script text ready to compile, in load order: libraries, the level, a test.
pub struct ScriptFiles {
    files: Vec<(String, String)>,
}

impl ScriptFiles {
    pub fn sources(&self) -> Vec<Source<'_>> {
        let files = self.files.iter();
        files.map(|(name, text)| Source { name, text }).collect()
    }
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
fn library(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut paths: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "st"))
        .collect();
    paths.sort();
    paths
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

/// The library a level at `level` would load, as names and text, for a host that
/// wants to look at it. With no level yet, the one in the source tree. A file
/// that cannot be read is left out: the level's own load will report it.
pub fn library_files(level: Option<&Path>) -> Vec<(String, String)> {
    let dir = level.map_or_else(|| library_dir(Path::new("")), library_dir);
    library(&dir).iter().filter_map(|p| read(p).ok()).collect()
}

fn read_all(paths: &[PathBuf]) -> Result<ScriptFiles, String> {
    let files = paths.iter().map(|p| read(p)).collect::<Result<_, _>>()?;
    Ok(ScriptFiles { files })
}

/// The scripts for a level, or `None` if it has none of its own.
pub fn for_level(level: &Path) -> Result<Option<ScriptFiles>, String> {
    let script = level.with_extension("st");
    if !script.is_file() {
        return Ok(None);
    }
    let mut paths = library(&library_dir(level));
    paths.push(script);
    read_all(&paths).map(Some)
}

/// The scripts for a test run: the library, the level's own if it has one, and
/// the test last.
pub fn for_test(level: &Path, test: &Path) -> Result<ScriptFiles, String> {
    let mut paths = library(&library_dir(level));
    let script = level.with_extension("st");
    if script.is_file() {
        paths.push(script);
    }
    paths.push(test.to_path_buf());
    read_all(&paths)
}

fn load_errors(errors: &[ScriptError]) -> String {
    let lines: Vec<String> = errors.iter().map(ToString::to_string).collect();
    format!("the level's script did not load:\n{}", lines.join("\n"))
}

/// Compiles and starts them, failing with every error found. Returns the
/// compiler's warnings.
pub fn start(session: &mut GameSession, scripts: &ScriptFiles) -> Result<Vec<ScriptError>, String> {
    session
        .load_script(&scripts.sources())
        .map_err(|errors| load_errors(&errors))
}

/// What a script file looked like last time.
type Stamp = Vec<(PathBuf, u64, Option<SystemTime>)>;

/// Watches a level's scripts for changes.
#[derive(Clone, Debug)]
pub struct ScriptWatcher {
    level: PathBuf,
    last: Stamp,
}

impl ScriptWatcher {
    /// Starts from the files as they are now, so nothing reloads at once.
    pub fn new(level: &Path) -> Self {
        let mut watcher = Self {
            level: level.to_path_buf(),
            last: Vec::new(),
        };
        watcher.last = watcher.stamp();
        watcher
    }

    fn stamp(&self) -> Stamp {
        let mut paths = library(&library_dir(&self.level));
        paths.push(self.level.with_extension("st"));
        let stamp_of = |path: PathBuf| {
            let meta = std::fs::metadata(&path).ok();
            let len = meta.as_ref().map_or(0, std::fs::Metadata::len);
            let time = meta.and_then(|m| m.modified().ok());
            (path, len, time)
        };
        paths.into_iter().map(stamp_of).collect()
    }

    /// Whether anything changed since the last call.
    pub fn changed(&mut self) -> bool {
        let now = self.stamp();
        let changed = now != self.last;
        self.last = now;
        changed
    }

    /// Reloads if anything changed. A level starts over on a script that loads.
    /// One that does not leaves the old script running and returns the problem,
    /// for the host to show. `Ok(true)` means it reloaded.
    pub fn reload_if_changed(&mut self, session: &mut GameSession) -> Result<bool, String> {
        if !self.changed() {
            return Ok(false);
        }
        let Some(files) = for_level(&self.level)? else {
            return Ok(false);
        };
        session
            .load_script(&files.sources())
            .map_err(|errors| load_errors(&errors))?;
        Ok(true)
    }
}
