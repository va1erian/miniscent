//! The lit view: a tile under a light that orbits it, shaded by the function the
//! game's shader is held to, so what shows here is what a level shows. `lightpanel`
//! is emissive by id in the game, drawn flat with no bump or highlight, and is here.
//!
//! The tile is drawn at most 512 px wide: this runs at 25 frames a second on a
//! worker, and a 1024 px map in a preview this size is minified anyway.

use mscent_math::Vec3;
use mscent_render::recipe_texture::texture_from_maps;
use mscent_render::shade::mul_rgb;
use mscent_render::specular::half_vector;
use mscent_render::texel_light::{TexelLight, TexelSurface, dither_of, shade_texel};
use mscent_render::{EMISSIVE, SpecTable, Texture, TextureError};
use mscent_texgen::Maps;

use super::eval::{Picture, Tab, over_backdrop};

/// Longest side drawn.
const MAX_SIDE: u32 = 512;
/// Light level at the texture's plain colour, 256 = full; a bright lamp.
const LEVEL: f32 = 300.0;
/// Radians the light moves per frame.
const ORBIT_STEP: f32 = 0.06;
/// How far from straight above the light hangs: its sine and cosine.
const TILT: (f32, f32) = (0.6, 0.8);

/// Straight above the tile, where the eye is.
const EYE: Vec3 = Vec3::new(0.0, 0.0, 1.0);

/// A material made ready to draw again and again.
pub struct LitTile {
    texture: Texture,
    /// What the game draws with: nothing for a gloss too low to show.
    table: Option<SpecTable>,
    emissive: bool,
    /// The request it answers.
    seq: u64,
    full: bool,
}

impl LitTile {
    pub fn new(
        maps: Maps,
        gloss: u8,
        emissive: bool,
        seq: u64,
        full: bool,
    ) -> Result<Self, TextureError> {
        let table = SpecTable::new(gloss);
        Ok(Self {
            texture: texture_from_maps(maps)?,
            table: table.is_visible().then_some(table),
            emissive,
            seq,
            full,
        })
    }

    /// The light at frame `step` of its orbit.
    fn light(step: u32) -> TexelLight {
        let angle = (step % 1024) as f32 * ORBIT_STEP;
        let dir = Vec3::new(angle.cos() * TILT.0, angle.sin() * TILT.0, TILT.1);
        let level = LEVEL * dir.z;
        TexelLight {
            color: Vec3::new(level, level, level),
            dir,
            half: half_vector(dir, EYE),
        }
    }

    fn surface_at(&self, x: i32, y: i32) -> TexelSurface<'_> {
        let packed = self.texture.normal_texel(x, y);
        let mean = self.texture.spec_mean();
        // As the game's: a mean of 1 means no mask to read.
        let mask = match packed {
            Some(n) if mean < 1.0 => (n >> 24) as f32 / 255.0,
            _ => mean,
        };
        TexelSurface {
            normal: self.texture.normal(x, y),
            table: self.table.as_ref(),
            mask,
        }
    }

    /// Whether the picture moves with the light.
    pub fn animates(&self) -> bool {
        !self.emissive
    }

    /// The tile at frame `step` of the orbit.
    pub fn render(&self, step: u32) -> Picture {
        let (tw, th) = (self.texture.width(), self.texture.height());
        let (w, h) = (tw.min(MAX_SIDE), th.min(MAX_SIDE));
        let light = Self::light(step);
        let mut rgba = Vec::with_capacity((w * h * 4) as usize);
        for y in 0..h as i32 {
            for x in 0..w as i32 {
                let (tx, ty) = (x * (tw / w) as i32, y * (th / h) as i32);
                let texel = self.texture.texel(tx, ty);
                let out = if self.emissive {
                    mul_rgb(texel, EMISSIVE)
                } else {
                    shade_texel(texel, light, self.surface_at(tx, ty), dither_of(x, y))
                };
                let [_, r, g, b] = out.to_be_bytes();
                // A cutout is drawn over a backdrop, or its clear part would be
                // black and look like paint.
                let coverage = self.texture.alpha_at(tx, ty);
                rgba.extend_from_slice(&over_backdrop(
                    [r, g, b],
                    coverage,
                    x as u32,
                    y as u32,
                    (w, h),
                ));
            }
        }
        Picture {
            seq: self.seq,
            tab: Tab::Lit,
            width: w,
            height: h,
            rgba,
            note: None,
            full: self.full,
        }
    }
}

#[cfg(test)]
mod tests {
    use mscent_render::bump::decode_normal;

    use super::*;

    const WHITE: u32 = 0xffff_ffff;

    fn maps(normals: Option<Vec<u32>>, specular: Option<Vec<u8>>) -> Maps {
        Maps {
            width: 4,
            height: 4,
            albedo: vec![0xff80_6040; 16],
            normals,
            heights: None,
            specular,
            alpha: None,
        }
    }

    #[test]
    fn the_flat_case_reads_the_albedo_under_the_light_and_the_light_moves_a_bump() {
        // Tilted one way: its brightness must follow where the light has got to.
        let tilted = decode_normal(0xffc0_80ff).normalized().expect("unit");
        let packed = 0xff00_0000
            | ((tilted.x * 127.5 + 127.5) as u32) << 16
            | ((tilted.y * 127.5 + 127.5) as u32) << 8
            | (tilted.z * 127.5 + 127.5) as u32;
        let tile =
            LitTile::new(maps(Some(vec![packed; 16]), None), 0, false, 1, true).expect("tile");
        let red = |step| tile.render(step).rgba[0];
        // Half an orbit apart: towards the tilt, and away from it.
        let half = (core::f32::consts::PI / ORBIT_STEP) as u32;
        assert!(red(0) > red(half) + 8, "{} {}", red(0), red(half));
        assert!(tile.animates());
    }

    #[test]
    fn a_gloss_too_low_to_show_draws_no_highlight_as_in_the_game() {
        let tile = |gloss| LitTile::new(maps(None, None), gloss, false, 1, true).expect("tile");
        assert!(tile(0).table.is_none());
        assert!(tile(200).table.is_some());
    }

    #[test]
    fn a_mask_lowers_the_highlight_texel_by_texel() {
        let flat = 0xff80_80ff;
        let mut mask = vec![255u8; 16];
        mask[0] = 0;
        let tile = LitTile::new(maps(Some(vec![flat; 16]), Some(mask)), 255, false, 1, true)
            .expect("tile");
        // Straight above: the highlight is at the centre of its lobe.
        let picture = tile.render(0);
        let lit = |i: usize| picture.rgba[i * 4 + 2];
        assert!(lit(1) > lit(0) + 4, "{} {}", lit(1), lit(0));
    }

    #[test]
    fn lightpanel_is_flat_as_in_the_game() {
        let tile = LitTile::new(maps(None, None), 120, true, 1, true).expect("tile");
        let (a, b) = (tile.render(0), tile.render(30));
        assert_eq!(a.rgba, b.rgba, "no light moves over it");
        let [_, r, g, bl] = mul_rgb(0xff80_6040, EMISSIVE).to_be_bytes();
        assert_eq!(&a.rgba[..4], &[r, g, bl, 255]);
        assert!(!tile.animates());
    }

    #[test]
    fn a_cutout_is_drawn_over_a_backdrop_where_it_is_clear() {
        let mut m = maps(None, None);
        let mut alpha = vec![255u8; 16];
        alpha[0] = 0;
        m.alpha = Some(alpha);
        let picture = LitTile::new(m, 0, false, 1, true).expect("tile").render(0);
        // The clear texel is the backdrop's grey, the solid one is the colour.
        let (clear, solid) = (&picture.rgba[0..4], &picture.rgba[4..8]);
        assert_eq!(clear, [96, 96, 96, 255], "{clear:?}");
        assert!(solid[0] > solid[2] && solid != clear, "{solid:?}");
    }

    #[test]
    fn a_large_map_is_drawn_at_the_cap() {
        let big = Maps {
            width: 1024,
            height: 1024,
            albedo: vec![WHITE; 1024 * 1024],
            normals: None,
            heights: None,
            specular: None,
            alpha: None,
        };
        let picture = LitTile::new(big, 0, false, 1, true)
            .expect("tile")
            .render(0);
        assert_eq!((picture.width, picture.height), (512, 512));
        assert_eq!(picture.rgba.len(), 512 * 512 * 4);
    }
}
