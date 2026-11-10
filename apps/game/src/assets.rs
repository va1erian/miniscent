//! Finding and loading the files the game needs at run time.

use std::path::{Path, PathBuf};

use mscent_render::{MaterialTable, recipe_texture};
use mscent_world::Level;

/// Directories that may hold `<sub>/`, most specific first: the explicit
/// root, next to the executable, two levels above it (a cargo `target/<profile>`
/// directory), the working directory and the source tree this was built from.
fn candidates(explicit: Option<&Path>, sub: &str) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(root) = explicit {
        dirs.push(root.join(sub));
    }
    if let Some(exe_dir) = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(Path::to_path_buf))
    {
        dirs.push(exe_dir.join("assets").join(sub));
        dirs.push(exe_dir.join("../../assets").join(sub));
    }
    dirs.push(Path::new("assets").join(sub));
    dirs.push(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../assets")
            .join(sub),
    );
    dirs
}

/// Whether a directory holds materials [`MaterialTable::load_dir`] can read: a
/// manifest, any recipe, or the one PNG every stock level needs, which keeps an
/// unrelated folder of pictures from being taken for textures.
fn holds_materials(dir: &Path) -> bool {
    let has_recipe = || {
        std::fs::read_dir(dir).is_ok_and(|mut files| {
            files.any(|file| {
                file.is_ok_and(|f| {
                    let name = f.file_name();
                    name.to_string_lossy().ends_with(recipe_texture::EXTENSION)
                        && f.path().is_file()
                })
            })
        })
    };
    dir.join(mscent_render::manifest::FILE).is_file()
        || dir.join("brick.png").is_file()
        || has_recipe()
}

/// Loads the material table from the first candidate directory that has the
/// textures. The error lists where it looked.
pub fn load_materials(assets_root: Option<&Path>) -> Result<MaterialTable, String> {
    load_materials_from(assets_root).map(|(table, _)| table)
}

/// As [`load_materials`], and the directory the table came from, which is where
/// a material made in the editor is saved.
pub fn load_materials_from(assets_root: Option<&Path>) -> Result<(MaterialTable, PathBuf), String> {
    let dirs = candidates(assets_root, "textures");
    for dir in &dirs {
        if holds_materials(dir) {
            return MaterialTable::load_dir(dir)
                .map(|table| (table, dir.clone()))
                .map_err(|e| format!("cannot load textures from {}: {e:?}", dir.display()));
        }
    }
    let looked: Vec<String> = dirs.iter().map(|d| d.display().to_string()).collect();
    Err(format!(
        "assets/textures not found; looked in: {}. Run from the Miniscent folder,          or give the game --assets <dir>",
        looked.join(", ")
    ))
}

/// Loads the sound effects from the first candidate directory that has any,
/// and the files in it that could not be read. A missing directory is not an
/// error the game should stop for, so the caller decides what to do with it.
pub fn load_sounds(
    assets_root: Option<&Path>,
) -> Result<(mscent_audio::SoundBank, Vec<String>), String> {
    let dirs = candidates(assets_root, "sounds");
    for dir in dirs.iter().filter(|d| d.is_dir()) {
        let (bank, skipped) = mscent_audio::SoundBank::load_dir(dir)
            .map_err(|e| format!("cannot read sounds in {}: {e}", dir.display()))?;
        if !bank.is_empty() {
            return Ok((bank, skipped));
        }
    }
    let looked: Vec<String> = dirs.iter().map(|d| d.display().to_string()).collect();
    Err(format!(
        "assets/sounds not found; looked in: {}",
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
    fn the_directory_reported_is_the_one_the_table_came_from() {
        let (table, dir) = load_materials_from(None).expect("textures");
        assert!(dir.join(mscent_render::manifest::FILE).is_file());
        let again = MaterialTable::load_dir(&dir).expect("loads");
        assert_eq!(again.len(), table.len());
    }

    #[test]
    fn source_tree_sounds_cover_every_cue() {
        let (bank, skipped) = load_sounds(None).expect("sounds");
        assert!(skipped.is_empty(), "{skipped:?}");
        let sounds = crate::sound::Sounds::new(bank, |_| {});
        assert_eq!(sounds.missing(), Vec::<&str>::new());
    }

    #[test]
    fn a_folder_without_sounds_says_where_it_looked() {
        let root = std::env::temp_dir().join(format!("mscent-nosound-{}", std::process::id()));
        std::fs::create_dir_all(root.join("sounds")).expect("temp dir");
        let got = load_sounds(Some(&root));
        let _ = std::fs::remove_dir_all(&root);
        // An empty folder falls through to the source tree's, never panics.
        assert!(got.is_ok());
    }

    #[test]
    fn missing_files_give_messages_not_panics() {
        let e = load_level(Path::new("no/such/level.msl")).unwrap_err();
        assert!(e.contains("cannot read"), "{e}");
        let e = load_materials(Some(Path::new("no/such/root")));
        assert!(e.is_ok(), "falls through to the source tree");
    }

    /// An `--assets` root holding `textures/` with the given files, whose table
    /// is reported with the directory it came from.
    fn root_with(tag: &str, files: &[(&str, &[u8])]) -> (PathBuf, MaterialTable, PathBuf) {
        let root = std::env::temp_dir().join(format!("mscent-assets-{tag}-{}", std::process::id()));
        let dir = root.join("textures");
        std::fs::create_dir_all(&dir).expect("temp dir");
        for (name, bytes) in files {
            std::fs::write(dir.join(name), bytes).expect("write");
        }
        let (table, from) = load_materials_from(Some(&root)).expect("loads");
        (root, table, from)
    }

    const RECIPE: &[u8] = b"miniscent-recipe 1\nsize 4 4\nop 0 0 1 flat\nop 0 1 1 out-albedo\n";

    #[test]
    fn a_root_with_a_manifest_is_used() {
        let files: [(&str, &[u8]); 2] = [("materials.txt", b"0 mine 7\n"), ("mine.mst", RECIPE)];
        let (root, table, from) = root_with("manifest", &files);
        assert_eq!(from, root.join("textures"));
        assert_eq!(table.len(), 1);
        assert!(table.find("mine").is_some());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_root_with_only_a_recipe_is_used_not_skipped_for_the_stock_folder() {
        let (root, table, from) = root_with("recipe", &[("brick.mst", RECIPE)]);
        assert_eq!(from, root.join("textures"));
        assert_eq!(table.len(), 1, "only its own brick");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_root_with_only_a_brick_png_is_used_not_skipped_for_the_stock_folder() {
        let png = mscent_render::png_encode::encode_png(2, 2, &[0xff00_0000; 4]).expect("encodes");
        let (root, table, from) = root_with("png", &[("brick.png", &png)]);
        assert_eq!(from, root.join("textures"));
        assert_eq!(table.len(), 1, "only its own brick");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_root_with_neither_falls_through_to_the_stock_folder() {
        let (root, table, from) = root_with("neither", &[("notes.txt", b"hi"), ("wall.png", b"x")]);
        assert_ne!(from, root.join("textures"));
        assert!(table.find("brick").is_some());
        let _ = std::fs::remove_dir_all(root);
    }
}
