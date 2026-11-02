//! Textures.
//!
//! A texture is a power-of-two square or rectangle of `0xAARRGGBB` texels so
//! the rasterizer can wrap coordinates with a mask. Optional companions: a
//! tangent-space normal map (`name_n.png`), a height map (`name_h.png`) and a
//! specular map (`name_s.png`), which is kept in the alpha of the normal texels
//! because the specular path fetches those anyway.

use core::fmt;
use std::path::Path;

use mscent_math::Vec3;

use crate::alpha::{AlphaPlane, AlphaSampler};
use crate::manifest::ManifestError;
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
    /// A specular map came with no normal map, whose alpha is where it lives.
    SpecularWithoutNormals,
    /// `materials.txt` is not acceptable.
    Manifest(ManifestError),
    /// A recipe could not be read or run.
    Recipe(String),
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
    /// Packed normals, with the specular mask in their alpha.
    normals: Option<Vec<u32>>,
    heights: Option<Vec<u8>>,
    /// Coverage of a cutout texture; absent when every texel is solid.
    alpha: Option<AlphaPlane>,
    /// Mean of the specular mask, 0 to 255, for where it is not fetched.
    spec_mean: u8,
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
            alpha: None,
            spec_mean: u8::MAX,
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
            alpha: None,
            spec_mean: u8::MAX,
        }
    }

    /// Turn on smooth sampling: bilinear magnification and a mip chain for
    /// minification. Plain textures sample the nearest texel, which keeps
    /// exact-texel tests and pixel-art uses simple; loaded materials are
    /// filtered. A texture that is already filtered keeps its chain.
    pub fn filtered(mut self) -> Self {
        if self.filtered {
            return self;
        }
        self.mips = mips::build(self.width, self.height, &self.albedo);
        self.alpha = self
            .alpha
            .take()
            .map(|a| a.with_mips(self.width, self.height));
        self.filtered = true;
        self
    }

    /// Whether [`Self::filtered`] was applied.
    pub fn is_filtered(&self) -> bool {
        self.filtered
    }

    /// Attach a normal map (`0xAARRGGBB`, xyz packed as `n * 0.5 + 0.5`). The
    /// alpha is the specular mask, so a map with no mask must have alpha 255.
    pub fn with_normals(mut self, normals: Vec<u32>) -> Result<Self, TextureError> {
        if normals.len() != self.albedo.len() {
            return Err(TextureError::SizeMismatch);
        }
        self.spec_mean = mean_byte(normals.iter().map(|&n| (n >> 24) as u8));
        self.normals = Some(normals);
        Ok(self)
    }

    /// Attach a specular mask, one byte per texel, 255 for the full highlight.
    /// It needs a normal map to live in: a made-up flat one would not be quite
    /// flat at 8 bits, and would cost the diffuse path a normal fetch per pixel.
    pub fn with_specular(mut self, mask: &[u8]) -> Result<Self, TextureError> {
        if mask.len() != self.albedo.len() {
            return Err(TextureError::SizeMismatch);
        }
        let Some(normals) = &mut self.normals else {
            return Err(TextureError::SpecularWithoutNormals);
        };
        for (n, &m) in normals.iter_mut().zip(mask) {
            *n = (*n & 0x00ff_ffff) | u32::from(m) << 24;
        }
        self.spec_mean = mean_byte(mask.iter().copied());
        Ok(self)
    }

    /// Mean of the specular mask, 0.0 to 1.0, rounded down: exactly 1.0 only for
    /// a texture with no mask or a full one, whose highlight must not change.
    pub fn spec_mean(&self) -> f32 {
        f32::from(self.spec_mean) / 255.0
    }

    /// Attach an 8-bit height map.
    pub fn with_heights(mut self, heights: Vec<u8>) -> Result<Self, TextureError> {
        if heights.len() != self.albedo.len() {
            return Err(TextureError::SizeMismatch);
        }
        self.heights = Some(heights);
        Ok(self)
    }

    /// Attach coverage, one byte per texel, 255 solid. A plane with no texel
    /// below 255 is dropped: the texture stays opaque and keeps the plain path.
    /// The colour's own alpha is untouched, and so are the maps' (the
    /// specular mask is the normal map's, not this).
    pub fn with_alpha(mut self, alpha: Vec<u8>) -> Result<Self, TextureError> {
        if alpha.len() != self.albedo.len() {
            return Err(TextureError::SizeMismatch);
        }
        self.alpha = AlphaPlane::new(alpha);
        if self.filtered {
            self.alpha = self
                .alpha
                .take()
                .map(|a| a.with_mips(self.width, self.height));
        }
        Ok(self)
    }

    /// Whether some texels are see-through or partly so: the cutout path.
    pub fn is_masked(&self) -> bool {
        self.alpha.is_some()
    }

    /// Coverage sampler over mip `level` (clamped), `None` for an opaque texture.
    pub fn alpha_sampler(&self, level: u32) -> Option<AlphaSampler<'_>> {
        Some(self.alpha.as_ref()?.sampler(self.width, self.height, level))
    }

    /// Coverage of a base texel with wrap-around addressing; 255 when opaque.
    pub fn alpha_at(&self, x: i32, y: i32) -> u8 {
        self.alpha
            .as_ref()
            .map_or(u8::MAX, |a| a.at(self.width, self.height, x, y))
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

    /// A bilinear sampler over mip `level` (clamped to the chain), for
    /// callers that fetch many texels from one level: it does the level
    /// lookup once.
    pub fn lod_sampler(&self, level: u32) -> LodSampler<'_> {
        match (level as usize).min(self.mips.len()).checked_sub(1) {
            None => LodSampler {
                px: &self.albedo,
                w: self.width,
                h: self.height,
                shift: 0,
            },
            Some(l) => {
                let shift = l as u32 + 1;
                LodSampler {
                    px: &self.mips[l],
                    w: (self.width >> shift).max(1),
                    h: (self.height >> shift).max(1),
                    shift,
                }
            }
        }
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

    /// The normal map as a cheap view for per-pixel fetches, if attached.
    pub fn normal_view(&self) -> Option<NormalView<'_>> {
        Some(NormalView {
            px: self.normals.as_deref()?,
            w: self.width as usize,
            mx: self.mask_x(),
            my: self.mask_y(),
        })
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

/// A texture's packed normal map, borrowed for the duration of a draw.
#[derive(Clone, Copy)]
pub struct NormalView<'a> {
    px: &'a [u32],
    w: usize,
    mx: u32,
    my: u32,
}

impl NormalView<'_> {
    /// Packed normal texel with wrap-around addressing; a short map reads as
    /// the flat normal.
    #[inline]
    pub fn texel(&self, x: i32, y: i32) -> u32 {
        let i = (y as u32 & self.my) as usize * self.w + (x as u32 & self.mx) as usize;
        self.px.get(i).copied().unwrap_or(0xff80_80ff)
    }
}

/// One mip level of a texture, sampled at full-resolution 16.16 coordinates.
#[derive(Clone, Copy)]
pub struct LodSampler<'a> {
    px: &'a [u32],
    w: u32,
    h: u32,
    shift: u32,
}

impl LodSampler<'_> {
    /// Same result as [`Texture::sample_bilinear_lod`] for this level.
    #[inline]
    pub fn bilinear(&self, fu: i32, fv: i32) -> u32 {
        bilinear(self.px, self.w, self.h, fu >> self.shift, fv >> self.shift)
    }

    /// Same result as [`Texture::texel_lod`] for this level.
    #[inline]
    pub fn nearest(&self, x: i32, y: i32) -> u32 {
        let (xm, ym) = (
            (x as u32 >> self.shift) & (self.w - 1),
            (y as u32 >> self.shift) & (self.h - 1),
        );
        self.px
            .get(ym as usize * self.w as usize + xm as usize)
            .copied()
            .unwrap_or(0)
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

/// Mean of `bytes`, rounded down, so it is 255 only if every byte is; 255 when
/// there are none.
fn mean_byte(bytes: impl ExactSizeIterator<Item = u8>) -> u8 {
    let n = bytes.len() as u64;
    if n == 0 {
        return u8::MAX;
    }
    // Under 2^56 even for 2^48 bytes, far beyond any texture.
    let sum: u64 = bytes.map(u64::from).sum();
    u8::try_from(sum / n).unwrap_or(u8::MAX)
}

/// Make the recipe `dir/name.mst` if there is one. Otherwise load `dir/name.png`
/// plus `name_n.png`, `name_h.png` and `name_s.png` when they exist. The alpha of
/// a normal map file means nothing and is dropped, so only `name_s.png` can mask a
/// highlight.
pub fn load_texture(dir: &Path, name: &str) -> Result<Texture, TextureError> {
    let recipe = dir.join(format!("{name}{}", crate::recipe_texture::EXTENSION));
    if recipe.is_file() {
        return crate::recipe_texture::load_recipe(&recipe);
    }
    let base = read_png(&dir.join(format!("{name}.png")))?;
    let texels = base.to_argb();
    let alpha: Vec<u8> = texels.iter().map(|&t| (t >> 24) as u8).collect();
    let mut tex = Texture::new(base.width, base.height, texels)?.with_alpha(alpha)?;
    let companion = |suffix: &str| -> Result<Option<Image>, TextureError> {
        let path = dir.join(format!("{name}_{suffix}.png"));
        if !path.is_file() {
            return Ok(None);
        }
        let image = read_png(&path)?;
        if (image.width, image.height) != (base.width, base.height) {
            return Err(TextureError::SizeMismatch);
        }
        Ok(Some(image))
    };
    if let Some(n) = companion("n")? {
        let mut texels = n.to_argb();
        for t in &mut texels {
            *t |= 0xff00_0000;
        }
        tex = tex.with_normals(texels)?;
    }
    if let Some(h) = companion("h")? {
        tex = tex.with_heights(h.first_channel())?;
    }
    if let Some(s) = companion("s")? {
        tex = tex.with_specular(&s.first_channel())?;
    }
    Ok(tex)
}

#[cfg(test)]
#[path = "texture_tests.rs"]
mod texture_tests;
