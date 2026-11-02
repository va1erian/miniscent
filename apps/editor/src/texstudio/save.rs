//! Writing a material to disk: its recipe and its row in `materials.txt`, each by
//! temp file and rename, so a failure leaves the old file whole. A recipe is proved
//! to read back before anything is written, and a manifest to parse, since a bad
//! row would stop the game from loading any material at all.

use std::io::Write;
use std::path::Path;

use mscent_render::manifest::{self, Entry, name_is_valid};
use mscent_render::recipe_texture::EXTENSION;
use mscent_render::{MaterialId, MaterialTable};
use mscent_texgen::Recipe;
use mscent_texgen::recipe;

fn io(path: &Path) -> impl Fn(std::io::Error) -> String + '_ {
    move |e| format!("{}: {e}", path.display())
}

/// Writes `bytes` to `path` through a temp file next to it. The temp file is on
/// the disk before the rename: after a power cut the rename can otherwise be the
/// only part that survived, leaving an empty manifest that stops all loading.
pub(crate) fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let temp = path.with_extension(format!("tmp{}", std::process::id()));
    let write = || -> std::io::Result<()> {
        let mut file = std::fs::File::create(&temp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        std::fs::rename(&temp, path)
    };
    let result = write().map_err(io(path));
    if result.is_err() {
        let _ = std::fs::remove_file(&temp);
    }
    result
}

/// The manifest's text now: the file, or for a directory with no manifest one that
/// lists what is there, which is the ids it had.
fn current_manifest(dir: &Path) -> Result<String, String> {
    let path = dir.join(manifest::FILE);
    match manifest::read_text(&path).map_err(|e| format!("{e:?}"))? {
        Some(text) => Ok(text),
        None => Ok(manifest::text_of(
            &manifest::from_listing(dir).map_err(|e| format!("{e:?}"))?,
        )),
    }
}

fn with_material(dir: &Path, text: &str, name: &str, gloss: u8) -> Result<(String, u16), String> {
    manifest::with_material(text, name, gloss).map_err(|e| {
        format!(
            "{} would not load: {e:?}",
            dir.join(manifest::FILE).display()
        )
    })
}

/// The manifest's text with `name` at `gloss` in it, and the material's id.
pub(crate) fn updated_manifest(dir: &Path, name: &str, gloss: u8) -> Result<(String, u16), String> {
    with_material(dir, &current_manifest(dir)?, name, gloss)
}

fn is_listed(manifest_text: &str, name: &str) -> bool {
    let entries = manifest::parse(manifest_text.as_bytes()).unwrap_or_default();
    entries
        .iter()
        .any(|e| matches!(e, Entry::Material { name: n, .. } if n == name))
}

/// The recipe's text, and the recipe read back from it. That is what the file will
/// hold and what the game will make from it, so it is what Save proves a page with.
pub(super) fn proved(recipe: &Recipe) -> Result<(String, Recipe), String> {
    let text = recipe::write(recipe);
    match recipe::read(text.as_bytes()) {
        Ok(back) if back == *recipe => Ok((text, back)),
        Ok(_) => Err("the recipe does not read back the same".to_string()),
        Err(e) => Err(format!("the recipe does not read back: {e}")),
    }
}

/// Whether `name` would have on disk, at `disk`, the id it has in the running
/// table. Levels drawn here store that number, so a file that disagrees would
/// show another material, or none, when they load.
fn id_agrees(name: &str, table: &MaterialTable, disk: u16) -> Result<(), String> {
    let here = table.find(name).map_or(table.len(), |id| usize::from(id.0));
    if usize::from(disk) == here {
        return Ok(());
    }
    let earlier: Vec<&str> = (disk..)
        .take_while(|&i| usize::from(i) < here)
        .filter_map(|i| table.get(MaterialId(i)).map(|m| m.name.as_str()))
        .collect();
    if earlier.is_empty() {
        return Err(format!(
            "{name} is id {here} here but id {disk} on disk: {} changed since this editor \
             loaded it",
            manifest::FILE
        ));
    }
    Err(format!(
        "{name} is id {here} here but would be id {disk} on disk: save {} first",
        earlier.join(", ")
    ))
}

/// Saves a material in `dir`. `table` is the running table: the id the material has
/// there is the one levels drawn here store, so the file must give it the same. It
/// returns the id. On any error the directory is as it was.
pub fn save_material(
    dir: &Path,
    name: &str,
    recipe: &Recipe,
    table: &MaterialTable,
) -> Result<u16, String> {
    if !name_is_valid(name) {
        return Err(format!("{name:?} cannot be a material's name"));
    }
    let (text, _) = proved(recipe)?;
    // Read now, not when the material was opened: another process may have added rows.
    let old_manifest = current_manifest(dir)?;
    let (manifest_text, id) = with_material(dir, &old_manifest, name, recipe.gloss)?;
    id_agrees(name, table, id)?;
    let path = dir.join(format!("{name}{EXTENSION}"));
    // A file nothing lists is someone's: a new row would adopt it and the write
    // would replace it.
    if !is_listed(&old_manifest, name) && path.try_exists().map_err(io(&path))? {
        return Err(format!(
            "{} exists but {} does not list it: rename or list it first",
            path.display(),
            manifest::FILE
        ));
    }
    // Only a missing file means there was none to put back.
    let previous = match std::fs::read(&path) {
        Ok(bytes) => Some(bytes),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(io(&path)(e)),
    };
    write_atomic(&path, text.as_bytes())?;
    if let Err(why) = write_atomic(&dir.join(manifest::FILE), manifest_text.as_bytes()) {
        // No half-saves: a row with no file and a file with no row both surprise.
        let _ = match previous {
            Some(bytes) => write_atomic(&path, &bytes),
            None => std::fs::remove_file(&path).map_err(|e| e.to_string()),
        };
        return Err(why);
    }
    Ok(id)
}

#[cfg(test)]
#[path = "save_tests.rs"]
mod save_tests;
