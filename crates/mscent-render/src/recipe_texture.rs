//! Textures made from recipes (`name.mst`), the form stock materials ship in: a
//! few kilobytes of operators in place of megabytes of pixels.

use std::io::Read;
use std::path::Path;

use mscent_texgen::pack::bake_cost;
use mscent_texgen::recipe::{self, MAX_FILE, READ_NS_PER_BYTE};
use mscent_texgen::{Maps, Recipe, bake};

use crate::manifest::Entry;
use crate::texture::{Texture, TextureError};

/// Extension of a recipe file, with its dot.
pub const EXTENSION: &str = ".mst";

/// Most nanoseconds of estimated work a directory's recipes may take together:
/// the worst directory that passes takes about half a minute on the machine the
/// costs were measured on, however many rows it lists. The game makes them at
/// startup, on one thread, with no way to cancel.
pub const MAX_DIRECTORY_NS: u64 = 30_000_000_000;

/// A texture from finished maps, not yet filtered.
pub fn texture_from_maps(maps: Maps) -> Result<Texture, TextureError> {
    let mut texture = Texture::new(maps.width, maps.height, maps.albedo)?;
    if let Some(normals) = maps.normals {
        texture = texture.with_normals(normals)?;
    }
    if let Some(heights) = maps.heights {
        texture = texture.with_heights(heights)?;
    }
    if let Some(specular) = maps.specular {
        texture = texture.with_specular(&specular)?;
    }
    Ok(texture)
}

fn recipe_error(path: &Path, e: mscent_texgen::TexError) -> TextureError {
    TextureError::Recipe(format!("{}: {e}", path.display()))
}

/// The recipe at `path` and its file's length. The file is untrusted, so it is
/// read no further than a recipe may be long.
fn read_recipe(path: &Path) -> Result<(Recipe, u64), TextureError> {
    let io = |e: std::io::Error| TextureError::Io(format!("{}: {e}", path.display()));
    let file = std::fs::File::open(path).map_err(io)?;
    let mut bytes = Vec::new();
    file.take(MAX_FILE as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(io)?;
    let recipe = recipe::read(&bytes).map_err(|e| recipe_error(path, e))?;
    Ok((recipe, bytes.len() as u64))
}

/// Reads the recipe at `path` and makes its texture.
pub fn load_recipe(path: &Path) -> Result<Texture, TextureError> {
    let (recipe, _) = read_recipe(path)?;
    texture_from_maps(bake(&recipe, recipe.size).map_err(|e| recipe_error(path, e))?)
}

/// Refuses a directory whose listed recipes would take too long together, or one
/// of which would be refused anyway, before any of them makes a pixel. Files are
/// read here and again when they are made, so reading is charged twice. PNGs have
/// their own limits and are not operator work, so they are not counted.
pub fn check_directory_work(dir: &Path, entries: &[Entry]) -> Result<(), TextureError> {
    let mut total = 0u64;
    for entry in entries {
        let Entry::Material { name, .. } = entry else {
            continue;
        };
        let path = dir.join(format!("{name}{EXTENSION}"));
        if !path.is_file() {
            continue;
        }
        let (recipe, bytes) = read_recipe(&path)?;
        let making = bake_cost(&recipe, recipe.size).map_err(|e| recipe_error(&path, e))?;
        total = total
            .saturating_add(2 * bytes * READ_NS_PER_BYTE)
            .saturating_add(making);
        if total > MAX_DIRECTORY_NS {
            return Err(TextureError::Recipe(format!(
                "{}: the recipes listed up to {name} take too much work together",
                dir.display()
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("mscent-recipe-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&d).expect("temp dir");
        d
    }

    #[test]
    fn a_recipe_makes_a_texture_with_its_maps() {
        let d = dir("maps");
        let path = d.join("a.mst");
        std::fs::write(
            &path,
            "miniscent-recipe 1
size 16 16
op 0 0 1 flat color=1,0,0
op 0 1 1 out-albedo
op 1 0 1 noise
op 1 1 1 out-height
op 2 0 1 flat color=0.5,0.5,0.5
op 2 1 1 out-specular
",
        )
        .expect("write");
        let t = load_recipe(&path).expect("loads");
        assert_eq!((t.width(), t.height()), (16, 16));
        assert_eq!(t.texel(3, 3), 0xffff_0000);
        assert!(t.has_normals() && t.has_heights());
        assert!((t.spec_mean() - 0.5).abs() < 0.01, "{}", t.spec_mean());
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn hostile_recipes_are_errors() {
        let d = dir("hostile");
        let path = d.join("a.mst");
        for bad in [
            "".to_string(),
            "miniscent-recipe 1\nsize 16 16\n".to_string(),
            "miniscent-recipe 1\nsize 16 16\nop 0 0 1 blur\nop 0 1 1 out-albedo\n".to_string(),
            " ".repeat(MAX_FILE + 2),
        ] {
            std::fs::write(&path, bad).expect("write");
            assert!(matches!(load_recipe(&path), Err(TextureError::Recipe(_))));
        }
        assert!(matches!(
            load_recipe(&d.join("missing.mst")),
            Err(TextureError::Io(_))
        ));
        let _ = std::fs::remove_dir_all(d);
    }

    /// Blocks of `count` eight-octave noises at 1024 px: dear, but not to check.
    fn dear(count: usize) -> String {
        let mut text = String::from("miniscent-recipe 1\nsize 1024 1024\n");
        for i in 0..count {
            text.push_str(&format!("op {} {} 1 noise octaves=8\n", i % 64, i / 64));
        }
        text.push_str("op 0 200 1 flat\nop 0 201 1 out-albedo\n");
        text
    }

    fn material(name: &str) -> Entry {
        Entry::Material {
            name: name.to_string(),
            gloss: 1,
        }
    }

    #[test]
    fn a_recipe_past_its_limit_is_refused_before_any_pixel() {
        let d = dir("one-recipe");
        let path = d.join("a.mst");
        std::fs::write(&path, dear(200)).expect("write");
        let Err(TextureError::Recipe(why)) = load_recipe(&path) else {
            panic!("refused");
        };
        assert!(why.contains("too large"), "{why}");
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn a_directory_past_the_total_limit_is_refused_before_any_pixel() {
        let d = dir("total");
        let entries: Vec<Entry> = (0..64).map(|i| material(&format!("m{i}"))).collect();
        for i in 0..64 {
            std::fs::write(d.join(format!("m{i}.mst")), dear(24)).expect("write");
        }
        let Err(TextureError::Recipe(why)) = check_directory_work(&d, &entries) else {
            panic!("refused");
        };
        assert!(why.contains("together"), "{why}");
        // One recipe of them is fine, and a row with no recipe costs nothing.
        assert!(check_directory_work(&d, &entries[..1]).is_ok());
        assert!(check_directory_work(&d, &[material("png-only"), Entry::Retired]).is_ok());
        let _ = std::fs::remove_dir_all(d);
    }

    /// Stock materials must never come near the limit, or a real game could trip
    /// it with its own files: all of them are under a hundredth of it.
    #[test]
    fn the_stock_recipes_are_far_below_the_limit() {
        let stock = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/textures");
        let entries = crate::manifest::read(&stock.join(crate::manifest::FILE))
            .expect("reads")
            .expect("a manifest");
        let mut total = 0;
        for name in mscent_world::materials::NAMES {
            let (recipe, _) =
                read_recipe(&stock.join(format!("{name}{EXTENSION}"))).expect("reads");
            total += bake_cost(&recipe, recipe.size).expect("costs");
        }
        assert!(total < MAX_DIRECTORY_NS / 100, "{total} ns");
        assert!(check_directory_work(&stock, &entries).is_ok());
    }
}
