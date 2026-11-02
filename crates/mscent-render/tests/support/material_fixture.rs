//! Texture directories made on the fly for `tests/material_files.rs` and
//! `tests/material_pictures.rs`.
//!
//! The stock materials ship as recipes, which `load_texture` prefers over PNGs.
//! These tests are about the PNG path (manifests, companion files, forged
//! files), so the fixture bakes the stock recipes once and writes the result as
//! PNGs: real stock content, read back through the loader under test.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use mscent_render::png_encode::encode_png;
use mscent_render::{MaterialTable, TextureError};
use mscent_texgen::{bake, recipe};

pub const STOCK: &str = "0 brick 25\n1 floor 90\n2 lightpanel 120\n3 metal 200\n4 rock 10\n";

pub fn stock_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/textures")
}

/// One gray value per texel, as the first channel of an opaque PNG.
fn gray(bytes: &[u8]) -> Vec<u32> {
    bytes
        .iter()
        .map(|&v| 0xff00_0000 | (u32::from(v) * 0x0001_0101))
        .collect()
}

/// The stock recipes baked and encoded as `(file name, PNG bytes)`: `name.png`
/// always, and `name_n.png`, `name_h.png`, `name_s.png` for the maps a recipe
/// makes.
fn stock_pngs() -> &'static [(String, Vec<u8>)] {
    static PNGS: OnceLock<Vec<(String, Vec<u8>)>> = OnceLock::new();
    PNGS.get_or_init(|| {
        let mut files = Vec::new();
        for entry in std::fs::read_dir(stock_dir()).expect("stock dir") {
            let path = entry.expect("entry").path();
            if path.extension().is_none_or(|e| e != "mst") {
                continue;
            }
            let name = path
                .file_stem()
                .expect("stem")
                .to_string_lossy()
                .into_owned();
            let recipe = recipe::read(&std::fs::read(&path).expect("recipe")).expect("parses");
            let maps = bake(&recipe, recipe.size).expect("bakes");
            let png =
                |pixels: &[u32]| encode_png(maps.width, maps.height, pixels).expect("encodes");
            files.push((format!("{name}.png"), png(&maps.albedo)));
            if let Some(n) = &maps.normals {
                files.push((format!("{name}_n.png"), png(n)));
            }
            if let Some(h) = &maps.heights {
                files.push((format!("{name}_h.png"), png(&gray(h))));
            }
            if let Some(s) = &maps.specular {
                files.push((format!("{name}_s.png"), png(&gray(s))));
            }
        }
        files
    })
}

/// A directory that goes away with the test.
pub struct Fixture(pub PathBuf);

impl Fixture {
    pub fn new(name: &str) -> Self {
        let dir =
            std::env::temp_dir().join(format!("mscent_materials_{}_{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("dir");
        Self(dir)
    }

    /// With the stock materials as PNGs, the masks of metal and floor included,
    /// and manifest `text`.
    pub fn with_stock(name: &str, text: &str) -> Self {
        let f = Self::new(name);
        for (file, bytes) in stock_pngs() {
            f.write(file, bytes);
        }
        f.write("materials.txt", text.as_bytes());
        f
    }

    pub fn write(&self, file: &str, bytes: &[u8]) {
        std::fs::write(self.0.join(file), bytes).expect("write");
    }

    pub fn png(&self, file: &str, side: u32, color: u32) {
        let px = vec![color; (side * side) as usize];
        self.write(file, &encode_png(side, side, &px).expect("encodes"));
    }

    pub fn load(&self) -> Result<MaterialTable, TextureError> {
        MaterialTable::load_dir(&self.0)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
