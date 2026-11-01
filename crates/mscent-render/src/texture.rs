//! Textures and the material table.
//!
//! A texture is a power-of-two square or rectangle of `0xAARRGGBB` texels so
//! the rasterizer can wrap coordinates with a mask. Optional companions: a
//! tangent-space normal map (`name_n.png`) and a height map (`name_h.png`).

use core::fmt;
use std::path::Path;

use mscent_math::Vec3;

use crate::png::{self, Image, PngError};

/// Why a texture could not be built or loaded.
#[derive(Debug)]
pub enum TextureError {
    /// Width or height is zero, not a power of two, or too large.
    BadSize,
    /// A companion map does not match the albedo size.
    SizeMismatch,
    /// The file could not be read.
    Io(String),
    /// The PNG could not be decoded.
    Png(PngError),
}

impl fmt::Display for TextureError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "texture: {self:?}")
    }
}

impl std::error::Error for TextureError {}

impl From<PngError> for TextureError {
    fn from(e: PngError) -> Self {
        Self::Png(e)
    }
}

/// A wrapping texture with optional normal and height maps.
#[derive(Clone, Debug)]
pub struct Texture {
    width: u32,
    height: u32,
    albedo: Vec<u32>,
    normals: Option<Vec<u32>>,
    heights: Option<Vec<u8>>,
}

impl Texture {
    /// Build from `0xAARRGGBB` texels. Sides must be powers of two.
    pub fn new(width: u32, height: u32, albedo: Vec<u32>) -> Result<Self, TextureError> {
        let ok = |n: u32| n.is_power_of_two() && n <= png::MAX_DIMENSION;
        let len = (width as usize).checked_mul(height as usize);
        if !ok(width) || !ok(height) || len != Some(albedo.len()) {
            return Err(TextureError::BadSize);
        }
        Ok(Self {
            width,
            height,
            albedo,
            normals: None,
            heights: None,
        })
    }

    /// A 1x1 texture of one colour: the flat-lit fallback.
    pub fn solid(color: u32) -> Self {
        Self {
            width: 1,
            height: 1,
            albedo: vec![color],
            normals: None,
            heights: None,
        }
    }

    /// Attach a normal map (`0xAARRGGBB`, xyz packed as `n * 0.5 + 0.5`).
    pub fn with_normals(mut self, normals: Vec<u32>) -> Result<Self, TextureError> {
        if normals.len() != self.albedo.len() {
            return Err(TextureError::SizeMismatch);
        }
        self.normals = Some(normals);
        Ok(self)
    }

    /// Attach an 8-bit height map.
    pub fn with_heights(mut self, heights: Vec<u8>) -> Result<Self, TextureError> {
        if heights.len() != self.albedo.len() {
            return Err(TextureError::SizeMismatch);
        }
        self.heights = Some(heights);
        Ok(self)
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    /// Mask that wraps a texel x coordinate.
    pub fn mask_x(&self) -> u32 {
        self.width - 1
    }

    /// Mask that wraps a texel y coordinate.
    pub fn mask_y(&self) -> u32 {
        self.height - 1
    }

    fn index(&self, x: i32, y: i32) -> usize {
        let xm = (x as u32 & self.mask_x()) as usize;
        let ym = (y as u32 & self.mask_y()) as usize;
        ym * self.width as usize + xm
    }

    /// Albedo texel with wrap-around addressing.
    pub fn texel(&self, x: i32, y: i32) -> u32 {
        self.albedo[self.index(x, y)]
    }

    /// Unit normal at a texel, if a normal map is attached.
    pub fn normal(&self, x: i32, y: i32) -> Option<Vec3> {
        let p = self.normals.as_ref()?[self.index(x, y)];
        let c = |shift: u32| f32::from(((p >> shift) & 0xff) as u8) / 127.5 - 1.0;
        let v = Vec3::new(c(16), c(8), c(0));
        Some(v.normalized().unwrap_or(Vec3::new(0.0, 0.0, 1.0)))
    }

    /// Packed `0xAARRGGBB` normal texel, undecoded: the per-pixel bump path
    /// decodes it inline instead of paying for a normalize.
    pub fn normal_texel(&self, x: i32, y: i32) -> Option<u32> {
        Some(self.normals.as_ref()?[self.index(x, y)])
    }

    /// Height at a texel, if a height map is attached.
    pub fn height_at(&self, x: i32, y: i32) -> Option<u8> {
        Some(self.heights.as_ref()?[self.index(x, y)])
    }

    pub fn has_normals(&self) -> bool {
        self.normals.is_some()
    }

    pub fn has_heights(&self) -> bool {
        self.heights.is_some()
    }
}

fn read_png(path: &Path) -> Result<Image, TextureError> {
    let bytes =
        std::fs::read(path).map_err(|e| TextureError::Io(format!("{}: {e}", path.display())))?;
    Ok(png::decode(&bytes)?)
}

/// Load `dir/name.png` plus `name_n.png` and `name_h.png` when they exist.
pub fn load_texture(dir: &Path, name: &str) -> Result<Texture, TextureError> {
    let base = read_png(&dir.join(format!("{name}.png")))?;
    let mut tex = Texture::new(base.width, base.height, base.to_argb())?;
    let same_size = |i: &Image| (i.width, i.height) == (base.width, base.height);
    let normal_path = dir.join(format!("{name}_n.png"));
    if normal_path.is_file() {
        let n = read_png(&normal_path)?;
        if !same_size(&n) {
            return Err(TextureError::SizeMismatch);
        }
        tex = tex.with_normals(n.to_argb())?;
    }
    let height_path = dir.join(format!("{name}_h.png"));
    if height_path.is_file() {
        let h = read_png(&height_path)?;
        if !same_size(&h) {
            return Err(TextureError::SizeMismatch);
        }
        tex = tex.with_heights(h.first_channel())?;
    }
    Ok(tex)
}

/// Index of a material in a [`MaterialTable`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct MaterialId(pub u16);

/// A named surface: its textures and how shiny it is.
#[derive(Clone, Debug)]
pub struct Material {
    pub name: String,
    /// Specular power input, 0 (matte) to 255 (mirror-like).
    pub gloss: u8,
    pub texture: Texture,
}

/// Default gloss per generated material; unknown names are mildly glossy.
pub fn default_gloss(name: &str) -> u8 {
    match name {
        "metal" => 200,
        "lightpanel" => 120,
        "floor" => 90,
        "brick" => 25,
        "rock" => 10,
        _ => 40,
    }
}

/// All materials of a level, addressed by [`MaterialId`].
#[derive(Clone, Debug, Default)]
pub struct MaterialTable {
    materials: Vec<Material>,
}

impl MaterialTable {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a material; `None` when the table is full (65536 entries).
    pub fn add(&mut self, material: Material) -> Option<MaterialId> {
        let id = u16::try_from(self.materials.len()).ok()?;
        self.materials.push(material);
        Some(MaterialId(id))
    }

    pub fn get(&self, id: MaterialId) -> Option<&Material> {
        self.materials.get(usize::from(id.0))
    }

    pub fn find(&self, name: &str) -> Option<MaterialId> {
        let i = self.materials.iter().position(|m| m.name == name)?;
        u16::try_from(i).ok().map(MaterialId)
    }

    pub fn len(&self) -> usize {
        self.materials.len()
    }

    pub fn is_empty(&self) -> bool {
        self.materials.is_empty()
    }

    /// Load every base texture in `dir`, sorted by name so ids are stable.
    /// Files starting with `_` and the `_n`/`_h`/`_s` companions are skipped.
    pub fn load_dir(dir: &Path) -> Result<Self, TextureError> {
        let io = |e: std::io::Error| TextureError::Io(format!("{}: {e}", dir.display()));
        let mut names = Vec::new();
        for entry in std::fs::read_dir(dir).map_err(io)? {
            let file = entry.map_err(io)?.file_name();
            let file = file.to_string_lossy();
            if let Some(stem) = file.strip_suffix(".png") {
                let companion = ["_n", "_h", "_s"].iter().any(|s| stem.ends_with(s));
                if !stem.starts_with('_') && !companion {
                    names.push(stem.to_string());
                }
            }
        }
        names.sort();
        let mut table = Self::new();
        for name in names {
            let texture = load_texture(dir, &name)?;
            let gloss = default_gloss(&name);
            table
                .add(Material {
                    name,
                    gloss,
                    texture,
                })
                .ok_or(TextureError::BadSize)?;
        }
        Ok(table)
    }
}

#[cfg(test)]
#[path = "texture_tests.rs"]
mod texture_tests;
