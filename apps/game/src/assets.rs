//! Finding and loading the files the game needs at run time.

use std::path::{Path, PathBuf};

use mscent_render::MaterialTable;
use mscent_world::Level;

/// Directories that may hold `textures/`, most specific first: the explicit
/// root, next to the executable, two levels above it (a cargo `target/<profile>`
/// directory), the working directory and the source tree this was built from.
fn candidates(explicit: Option<&Path>) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(root) = explicit {
        dirs.push(root.join("textures"));
    }
    if let Some(exe_dir) = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(Path::to_path_buf))
    {
        dirs.push(exe_dir.join("assets/textures"));
        dirs.push(exe_dir.join("../../assets/textures"));
    }
    dirs.push(PathBuf::from("assets/textures"));
    dirs.push(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/textures"));
    dirs
}

/// Loads the material table from the first candidate directory that has the
/// textures. The error lists where it looked.
pub fn load_materials(assets_root: Option<&Path>) -> Result<MaterialTable, String> {
    let dirs = candidates(assets_root);
    for dir in &dirs {
        if dir.join(mscent_render::manifest::FILE).is_file() {
            return MaterialTable::load_dir(dir)
                .map_err(|e| format!("cannot load textures from {}: {e:?}", dir.display()));
        }
    }
    let looked: Vec<String> = dirs.iter().map(|d| d.display().to_string()).collect();
    Err(format!(
        "assets/textures not found; looked in: {} (use --assets <dir>)",
        looked.join(", ")
    ))
}

/// Reads a `.msl` level file.
pub fn load_level(path: &Path) -> Result<Level, String> {
    let data = std::fs::read(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    mscent_world::format::read(&data)
        .map_err(|e| format!("{} is not a valid level: {e:?}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_tree_textures_are_found_and_complete() {
        let t = load_materials(None).expect("textures");
        for name in mscent_world::materials::NAMES {
            assert!(t.find(name).is_some(), "{name}");
        }
    }

    #[test]
    fn missing_files_give_messages_not_panics() {
        let e = load_level(Path::new("no/such/level.msl")).unwrap_err();
        assert!(e.contains("cannot read"), "{e}");
        let e = load_materials(Some(Path::new("no/such/root")));
        assert!(e.is_ok(), "falls through to the source tree");
    }
}
