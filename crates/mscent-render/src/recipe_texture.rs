//! Textures made from recipes (`name.mst`), the form stock materials ship in: a
//! few kilobytes of operators in place of megabytes of pixels.

use std::io::Read;
use std::path::Path;

use mscent_texgen::recipe::{self, MAX_FILE};
use mscent_texgen::{Maps, bake};

use crate::texture::{Texture, TextureError};

/// Extension of a recipe file, with its dot.
pub const EXTENSION: &str = ".mst";

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

/// Reads the recipe at `path` and makes its texture. The file is untrusted, so
/// it is read no further than a recipe may be long.
pub fn load_recipe(path: &Path) -> Result<Texture, TextureError> {
    let io = |e: std::io::Error| TextureError::Io(format!("{}: {e}", path.display()));
    let recipe_error =
        |e: mscent_texgen::TexError| TextureError::Recipe(format!("{}: {e}", path.display()));
    let file = std::fs::File::open(path).map_err(io)?;
    let mut bytes = Vec::new();
    file.take(MAX_FILE as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(io)?;
    let recipe = recipe::read(&bytes).map_err(recipe_error)?;
    texture_from_maps(bake(&recipe, recipe.size).map_err(recipe_error)?)
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
}
