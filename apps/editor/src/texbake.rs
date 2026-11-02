//! `--bake-texture <recipe> <dir>`: a recipe's maps as PNG files, for looking at
//! without a window, and for an agent writing a recipe. When `<dir>` has a
//! `materials.txt`, the material joins it, so an agent can make a material the game
//! loads with no window at all.

use std::path::Path;

use mscent_render::manifest;
use mscent_render::png_encode::{encode_png, encode_png_rgba};
use mscent_render::recipe_texture::EXTENSION;
use mscent_texgen::{Maps, bake};

use crate::texstudio::{read_recipe_file, updated_manifest, write_atomic};

fn grey(values: &[u8]) -> Vec<u32> {
    values
        .iter()
        .map(|&v| 0xff00_0000 | (u32::from(v) * 0x0001_0101))
        .collect()
}

fn write_png(path: &Path, maps: &Maps, pixels: &[u32]) -> Result<(), String> {
    write_bytes(path, encode_png(maps.width, maps.height, pixels))
}

fn write_bytes(path: &Path, bytes: Option<Vec<u8>>) -> Result<(), String> {
    let bytes = bytes.ok_or_else(|| format!("{}: cannot encode", path.display()))?;
    std::fs::write(path, bytes).map_err(|e| format!("{}: {e}", path.display()))
}

/// Writes `name.png` (with alpha, if the recipe has `out-alpha`) and whichever of
/// `name_n.png`, `name_h.png` and `name_s.png` the recipe has, where `name` is the
/// recipe file's stem.
pub fn run(recipe_path: &Path, dir: &Path) -> Result<(), String> {
    let recipe = read_recipe_file(recipe_path)?;
    let name = recipe_path
        .file_stem()
        .ok_or_else(|| format!("{}: no file name", recipe_path.display()))?
        .to_string_lossy();
    // Before anything is written: the name goes into file names and a manifest.
    if !manifest::name_is_valid(&name) {
        return Err(format!(
            "{}: {name:?} cannot be a material's name",
            recipe_path.display()
        ));
    }
    refuse_shadowing(recipe_path, dir, &name)?;
    let maps = bake(&recipe, recipe.size).map_err(|e| format!("{}: {e}", recipe_path.display()))?;
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    write_base_png(&dir.join(format!("{name}.png")), &maps)?;
    if let Some(normals) = &maps.normals {
        write_png(&dir.join(format!("{name}_n.png")), &maps, normals)?;
    }
    if let Some(heights) = &maps.heights {
        write_png(&dir.join(format!("{name}_h.png")), &maps, &grey(heights))?;
    }
    if let Some(specular) = &maps.specular {
        write_png(&dir.join(format!("{name}_s.png")), &maps, &grey(specular))?;
    }
    list_in_manifest(dir, &name, recipe.gloss)
}

/// The base picture, with the alpha in the PNG's own channel when the recipe has
/// one, which is where the loader reads it from.
fn write_base_png(path: &Path, maps: &Maps) -> Result<(), String> {
    let Some(alpha) = &maps.alpha else {
        return write_png(path, maps, &maps.albedo);
    };
    let texels: Vec<u32> = maps
        .albedo
        .iter()
        .zip(alpha)
        .map(|(&c, &a)| u32::from(a) << 24 | c & 0xff_ffff)
        .collect();
    write_bytes(path, encode_png_rgba(maps.width, maps.height, &texels))
}

/// A recipe of the same name in `dir` wins over the PNGs about to be written, so
/// the pictures would never be the material. The recipe being baked is no such
/// recipe when it is that very file.
fn refuse_shadowing(recipe_path: &Path, dir: &Path, name: &str) -> Result<(), String> {
    let there = dir.join(format!("{name}{EXTENSION}"));
    if !there.try_exists().unwrap_or(true) {
        return Ok(());
    }
    let same = match (
        std::fs::canonicalize(recipe_path),
        std::fs::canonicalize(&there),
    ) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    };
    if same {
        return Ok(());
    }
    Err(format!(
        "{} exists and is loaded instead of the PNGs this would write: remove it, or bake \
         into another directory",
        there.display()
    ))
}

/// A directory with a manifest is one a game reads, whose manifest is the whole
/// truth, so the material gets its row there; one without is left as a place to
/// look at pictures in.
fn list_in_manifest(dir: &Path, name: &str, gloss: u8) -> Result<(), String> {
    let path = dir.join(manifest::FILE);
    if !path.is_file() {
        return Ok(());
    }
    let (text, _) = updated_manifest(dir, name, gloss)?;
    write_atomic(&path, text.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    use mscent_render::{MaterialTable, load_texture};

    #[test]
    fn what_it_writes_loads_as_a_texture() {
        let dir = std::env::temp_dir().join(format!("mscent-bake-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dir");
        let source = dir.join("a.mst");
        std::fs::write(
            &source,
            "miniscent-recipe 1
size 16 16
op 0 0 1 flat color=0,1,0
op 0 1 1 out-albedo
op 1 0 1 noise
op 1 1 1 out-height
op 2 0 1 flat color=0.5,0.5,0.5
op 2 1 1 out-specular
",
        )
        .expect("write");
        let out = dir.join("out");
        run(&source, &out).expect("bakes");
        let t = load_texture(&out, "a").expect("loads");
        assert_eq!(t.texel(0, 0), 0xff00_ff00);
        assert!(t.has_normals() && t.has_heights());
        assert!((t.spec_mean() - 0.5).abs() < 0.01);
        assert!(run(&dir.join("missing.mst"), &out).is_err());
        assert!(!out.join(manifest::FILE).exists(), "none was asked for");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_directory_with_a_manifest_gets_a_row_the_game_can_load() {
        let dir = std::env::temp_dir().join(format!("mscent-bake-row-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("dir");
        let source = dir.join("wall.mst");
        let text = "miniscent-recipe 1\nsize 16 16\ngloss 33\nop 0 0 1 flat color=0,1,0\nop 0 1 1 out-albedo\n";
        std::fs::write(&source, text).expect("write");
        let out = dir.join("assets");
        std::fs::create_dir_all(&out).expect("dir");
        std::fs::write(out.join(manifest::FILE), "# ids\n").expect("write");
        // A file of comments alone is no manifest, and it says so.
        assert!(run(&source, &out).is_err());
        std::fs::write(out.join(manifest::FILE), "0 a 1\n").expect("write");
        std::fs::write(out.join("a.mst"), text).expect("write");
        run(&source, &out).expect("bakes");
        let table = MaterialTable::load_dir(&out).expect("the game can load it");
        let wall = table
            .find("wall")
            .and_then(|id| table.get(id))
            .expect("wall");
        assert_eq!(wall.gloss, 33);
        assert_eq!(wall.texture.texel(0, 0), 0xff00_ff00);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_masked_recipe_writes_a_png_whose_alpha_the_loader_reads() {
        let dir = scratch("alpha");
        let source = dir.join("cut.mst");
        let text = "miniscent-recipe 1
size 16 16
op 0 0 1 flat color=0,1,0
op 0 1 1 out-albedo
op 1 0 1 rect x=0.5 y=0.5 w=0.5 h=1
op 1 1 1 out-alpha
";
        std::fs::write(&source, text).expect("write");
        let out = dir.join("out");
        run(&source, &out).expect("bakes");
        let t = load_texture(&out, "cut").expect("loads");
        assert!(t.is_masked());
        assert_eq!((t.alpha_at(8, 8), t.alpha_at(0, 8)), (255, 0));
        let _ = std::fs::remove_dir_all(&dir);
    }

    const FLAT: &str = "miniscent-recipe 1\nsize 16 16\nop 0 0 1 flat\nop 0 1 1 out-albedo\n";

    fn scratch(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("mscent-bake-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("dir");
        dir
    }

    fn files_in(dir: &Path) -> usize {
        std::fs::read_dir(dir).map_or(0, Iterator::count)
    }

    #[test]
    fn a_name_that_cannot_be_a_material_writes_nothing() {
        let dir = scratch("name");
        let out = dir.join("out");
        std::fs::create_dir_all(&out).expect("dir");
        for bad in ["Wall.mst", "a_n.mst", "con.mst", "-x.mst"] {
            let source = dir.join(bad);
            std::fs::write(&source, FLAT).expect("write");
            assert!(run(&source, &out).is_err(), "{bad}");
        }
        assert_eq!(files_in(&out), 0, "no PNG was left behind");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_file_over_the_size_limit_is_refused_without_being_held() {
        let dir = scratch("big");
        let source = dir.join("big.mst");
        std::fs::write(&source, vec![b' '; mscent_texgen::recipe::MAX_FILE + 1]).expect("write");
        assert!(run(&source, &dir.join("out")).is_err());
        assert!(!dir.join("out").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_recipe_of_that_name_already_in_the_directory_is_said_so_and_nothing_is_written() {
        let dir = scratch("stale");
        let source = dir.join("wall.mst");
        std::fs::write(&source, FLAT).expect("write");
        let out = dir.join("out");
        std::fs::create_dir_all(&out).expect("dir");
        std::fs::write(out.join("wall.mst"), "an older one").expect("write");
        let why = run(&source, &out).expect_err("refused");
        assert!(why.contains("wall.mst") && why.contains("instead"), "{why}");
        assert_eq!(files_in(&out), 1, "only the older recipe");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
