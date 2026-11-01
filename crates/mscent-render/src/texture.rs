//! Textures and the material table.
//!
//! A texture is a power-of-two square or rectangle of `0xAARRGGBB` texels so
//! the rasterizer can wrap coordinates with a mask. Optional companions: a
//! tangent-space normal map (`name_n.png`) and a height map (`name_h.png`).

use core::fmt;
use std::path::Path;

use mscent_math::Vec3;

use crate::mips;
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
    /// Half-size, quarter-size... copies of `albedo` (see [`crate::mips`]).
    mips: Vec<Vec<u32>>,
    filtered: bool,
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
            mips: Vec::new(),
            filtered: false,
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
            mips: Vec::new(),
            filtered: false,
            normals: None,
            heights: None,
        }
    }

    /// Turn on smooth sampling: bilinear magnification and a mip chain for
    /// minification. Plain textures sample the nearest texel, which keeps
    /// exact-texel tests and pixel-art uses simple; loaded materials are
    /// filtered.
    pub fn filtered(mut self) -> Self {
        self.mips = mips::build(self.width, self.height, &self.albedo);
        self.filtered = true;
        self
    }

    /// Whether [`Self::filtered`] was applied.
    pub fn is_filtered(&self) -> bool {
        self.filtered
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

    /// Number of mip levels above the base one.
    pub fn mip_count(&self) -> u32 {
        self.mips.len() as u32
    }

    /// Albedo texel at full-resolution coordinates `(x, y)`, read from mip
    /// `level` (0 is the base, larger values are blurrier and are clamped).
    pub fn texel_lod(&self, x: i32, y: i32, level: u32) -> u32 {
        let Some(l) = (level as usize).min(self.mips.len()).checked_sub(1) else {
            return self.texel(x, y);
        };
        let (w, h) = (
            (self.width >> (l + 1)).max(1),
            (self.height >> (l + 1)).max(1),
        );
        let (xm, ym) = (
            (x as u32 >> (l + 1)) & (w - 1),
            (y as u32 >> (l + 1)) & (h - 1),
        );
        self.mips[l]
            .get(ym as usize * w as usize + xm as usize)
            .copied()
            .unwrap_or(0)
    }

    /// Bilinearly filtered base-level albedo at 16.16 fixed-point texel
    /// coordinates. Texel centres sit at half-integers, so an exact texel
    /// centre returns that texel unchanged. Wraps like [`Self::texel`].
    pub fn sample_bilinear(&self, fu: i32, fv: i32) -> u32 {
        bilinear(&self.albedo, self.width, self.height, fu, fv)
    }

    /// Bilinearly filtered albedo from mip `level` (clamped to the chain) at
    /// the same full-resolution 16.16 coordinates as [`Self::sample_bilinear`].
    pub fn sample_bilinear_lod(&self, fu: i32, fv: i32, level: u32) -> u32 {
        let Some(l) = (level as usize).min(self.mips.len()).checked_sub(1) else {
            return self.sample_bilinear(fu, fv);
        };
        let shift = l as u32 + 1;
        let (w, h) = ((self.width >> shift).max(1), (self.height >> shift).max(1));
        bilinear(&self.mips[l], w, h, fu >> shift, fv >> shift)
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

/// Bilinear fetch from a `w` x `h` (powers of two) image at 16.16 coordinates.
/// Out-of-range indices cannot happen (masked), but a short slice reads black.
fn bilinear(px: &[u32], w: u32, h: u32, fu: i32, fv: i32) -> u32 {
    let (u, v) = (fu.wrapping_sub(0x8000), fv.wrapping_sub(0x8000));
    let (x0, y0) = ((u >> 16) as u32, (v >> 16) as u32);
    let (mx, my) = (w - 1, h - 1);
    let w = w as usize;
    let (xa, xb) = ((x0 & mx) as usize, (x0.wrapping_add(1) & mx) as usize);
    let (ra, rb) = (
        (y0 & my) as usize * w,
        (y0.wrapping_add(1) & my) as usize * w,
    );
    let at = |i: usize| px.get(i).copied().unwrap_or(0xff00_0000);
    let (tx, ty) = (((u >> 8) & 0xff) as u32, ((v >> 8) & 0xff) as u32);
    let top = lerp_texel(at(ra + xa), at(ra + xb), tx);
    let bottom = lerp_texel(at(rb + xa), at(rb + xb), tx);
    lerp_texel(top, bottom, ty)
}

/// Blend two opaque texels by `t / 256`, two channels at a time.
fn lerp_texel(a: u32, b: u32, t: u32) -> u32 {
    let mix = |a: u32, b: u32| {
        (a.wrapping_mul(256 - t).wrapping_add(b.wrapping_mul(t)) >> 8) & 0x00ff_00ff
    };
    let rb = mix(a & 0x00ff_00ff, b & 0x00ff_00ff);
    let g = mix((a >> 8) & 0x00ff_00ff, (b >> 8) & 0x00ff_00ff);
    0xff00_0000 | rb | g << 8
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

    /// The same materials with smooth sampling on (see
    /// [`Texture::filtered`]). Costs memory for the mip chains and some
    /// time per pixel, which is why it is a choice and not the default.
    pub fn filtered(mut self) -> Self {
        for m in &mut self.materials {
            m.texture = std::mem::replace(&mut m.texture, Texture::solid(0)).filtered();
        }
        self
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
