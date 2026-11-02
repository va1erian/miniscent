//! Writing a material to disk: its recipe and its row in `materials.txt`, each by
//! temp file and rename, so a failure leaves the old file whole. A recipe is proved
//! to read back before anything is written, and a manifest to parse, since a bad
//! row would stop the game from loading any material at all.

use std::path::Path;

use mscent_render::manifest::{self, name_is_valid};
use mscent_render::recipe_texture::EXTENSION;
use mscent_texgen::Recipe;
use mscent_texgen::recipe;

fn io(path: &Path) -> impl Fn(std::io::Error) -> String + '_ {
    move |e| format!("{}: {e}", path.display())
}

/// Writes `bytes` to `path` through a temp file next to it.
pub(crate) fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let temp = path.with_extension(format!("tmp{}", std::process::id()));
    let result = std::fs::write(&temp, bytes)
        .and_then(|()| std::fs::rename(&temp, path))
        .map_err(io(path));
    if result.is_err() {
        let _ = std::fs::remove_file(&temp);
    }
    result
}

/// The manifest's text with `name` at `gloss` in it, and the material's id. A
/// directory with no manifest gets one that lists what is there now, which is the
/// ids it had.
pub(crate) fn updated_manifest(dir: &Path, name: &str, gloss: u8) -> Result<(String, u16), String> {
    let path = dir.join(manifest::FILE);
    let text = match manifest::read_text(&path).map_err(|e| format!("{e:?}"))? {
        Some(text) => text,
        None => manifest::text_of(&manifest::from_listing(dir).map_err(|e| format!("{e:?}"))?),
    };
    manifest::with_material(&text, name, gloss)
        .map_err(|e| format!("{} would not load: {e:?}", path.display()))
}

/// The recipe's text, proved to read back as the same recipe.
fn recipe_text(recipe: &Recipe) -> Result<String, String> {
    let text = recipe::write(recipe);
    match recipe::read(text.as_bytes()) {
        Ok(back) if back == *recipe => Ok(text),
        Ok(_) => Err("the recipe does not read back the same".to_string()),
        Err(e) => Err(format!("the recipe does not read back: {e}")),
    }
}

/// Saves a material in `dir`. `known_id` is its id in the running table, if it has
/// one: a level drawn in this editor uses that number, so the file must agree. It
/// returns the id. On any error the directory is as it was.
pub fn save_material(
    dir: &Path,
    name: &str,
    recipe: &Recipe,
    known_id: Option<u16>,
) -> Result<u16, String> {
    if !name_is_valid(name) {
        return Err(format!("{name:?} cannot be a material's name"));
    }
    let text = recipe_text(recipe)?;
    let (manifest_text, id) = updated_manifest(dir, name, recipe.gloss)?;
    if let Some(known) = known_id
        && known != id
    {
        return Err(format!(
            "{name} is id {known} here but would be id {id} on disk: save the materials \
             applied before it first"
        ));
    }
    let path = dir.join(format!("{name}{EXTENSION}"));
    let previous = std::fs::read(&path).ok();
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
