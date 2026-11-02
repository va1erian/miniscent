//! `--bake-texture <recipe> <dir>`: a recipe's maps as PNG files, for looking at
//! without a window, and for an agent writing a recipe.

use std::path::Path;

use mscent_render::png_encode::encode_png;
use mscent_texgen::{Maps, bake, recipe};

fn grey(values: &[u8]) -> Vec<u32> {
    values
        .iter()
        .map(|&v| 0xff00_0000 | (u32::from(v) * 0x0001_0101))
        .collect()
}

fn write_png(path: &Path, maps: &Maps, pixels: &[u32]) -> Result<(), String> {
    let bytes = encode_png(maps.width, maps.height, pixels)
        .ok_or_else(|| format!("{}: cannot encode", path.display()))?;
    std::fs::write(path, bytes).map_err(|e| format!("{}: {e}", path.display()))
}

/// Writes `name.png` and whichever of `name_n.png`, `name_h.png` and
/// `name_s.png` the recipe has, where `name` is the recipe file's stem.
pub fn run(recipe_path: &Path, dir: &Path) -> Result<(), String> {
    let bytes =
        std::fs::read(recipe_path).map_err(|e| format!("{}: {e}", recipe_path.display()))?;
    let recipe = recipe::read(&bytes).map_err(|e| format!("{}: {e}", recipe_path.display()))?;
    let maps = bake(&recipe, recipe.size).map_err(|e| format!("{}: {e}", recipe_path.display()))?;
    let name = recipe_path
        .file_stem()
        .ok_or_else(|| format!("{}: no file name", recipe_path.display()))?
        .to_string_lossy();
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    write_png(&dir.join(format!("{name}.png")), &maps, &maps.albedo)?;
    if let Some(normals) = &maps.normals {
        write_png(&dir.join(format!("{name}_n.png")), &maps, normals)?;
    }
    if let Some(heights) = &maps.heights {
        write_png(&dir.join(format!("{name}_h.png")), &maps, &grey(heights))?;
    }
    if let Some(specular) = &maps.specular {
        write_png(&dir.join(format!("{name}_s.png")), &maps, &grey(specular))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use mscent_render::load_texture;

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
        let _ = std::fs::remove_dir_all(&dir);
    }
}
