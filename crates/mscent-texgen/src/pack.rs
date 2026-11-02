//! Finished maps as the renderer stores them: `0xAARRGGBB` albedo, a tangent-space
//! normal map derived from the height, and 8-bit height and specular.

use crate::error::TexError;
use crate::graph::{self, Graph, Outcome, RunOptions};
use crate::image::{Image, Size};
use crate::ops::{Channel, Value};
use crate::recipe::Recipe;

/// Size at which `normal_strength` means what it says. A height difference per
/// texel halves when the resolution doubles, so relief is scaled from here.
const STRENGTH_SIZE: f32 = 128.0;
/// Estimated nanoseconds per texel of one map, from a kernel's image to the
/// renderer's, measured with the `recipe_limits` example.
const PACK_NS_PER_TEXEL: u64 = 6;

/// A material's maps, each `width * height` long.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Maps {
    pub width: u32,
    pub height: u32,
    pub albedo: Vec<u32>,
    /// `None` only when the recipe has neither a height nor a specular map.
    pub normals: Option<Vec<u32>>,
    pub heights: Option<Vec<u8>>,
    pub specular: Option<Vec<u8>>,
}

/// A stored channel as the renderer's 8 bits.
#[inline]
pub fn byte(v: u16) -> u8 {
    ((u32::from(v) * 255 + 32767) / 65535) as u8
}

fn packed_albedo(image: &Image) -> Vec<u32> {
    image
        .pixels()
        .iter()
        .map(|p| {
            0xff00_0000
                | u32::from(byte(p[0])) << 16
                | u32::from(byte(p[1])) << 8
                | u32::from(byte(p[2]))
        })
        .collect()
}

fn first_channel(image: &Image) -> Vec<u8> {
    image.pixels().iter().map(|p| byte(p[0])).collect()
}

#[inline]
fn packed_axis(v: f32) -> u32 {
    ((v * 0.5 + 0.5) * 255.0 + 0.5) as u32 & 0xff
}

/// Wrapped central differences of the 16-bit height, with the signs of the
/// original generator: `(-dx, dy, 1)`, normalized. It works before quantizing to
/// 8 bits, so gentle slopes do not stair-step.
pub fn normals_from_height(image: &Image, strength: f32) -> Vec<u32> {
    let size = image.size();
    let (sx, sy) = (
        strength * size.width() as f32 / STRENGTH_SIZE,
        strength * size.height() as f32 / STRENGTH_SIZE,
    );
    let h = |x: i32, y: i32| f32::from(image.at(x, y)[0]) / 65535.0;
    let (w, ht) = (size.width() as i32, size.height() as i32);
    let mut out = Vec::with_capacity(size.texels());
    for y in 0..ht {
        for x in 0..w {
            let dx = (h(x + 1, y) - h(x - 1, y)) * 0.5 * sx;
            let dy = (h(x, y + 1) - h(x, y - 1)) * 0.5 * sy;
            let length = (dx * dx + dy * dy + 1.0).sqrt();
            out.push(
                0xff00_0000
                    | packed_axis(-dx / length) << 16
                    | packed_axis(dy / length) << 8
                    | packed_axis(1.0 / length),
            );
        }
    }
    out
}

/// The strength on the page's `out-height`.
pub fn normal_strength(recipe: &Recipe, block: usize) -> f32 {
    let values = recipe.blocks.get(block).map(|b| b.params.values());
    values.and_then(|v| v.first()).map_or(0.0, |v| match v {
        Value::Float(s) => *s,
        _ => 0.0,
    })
}

/// A finished run as maps. A page with no `out-albedo` has no material.
pub fn pack(recipe: &Recipe, graph: &Graph, outcome: &Outcome) -> Result<Maps, TexError> {
    let map = |channel| {
        let block = graph.output(channel)?;
        Some((block, outcome.image(block)?))
    };
    let (_, albedo) =
        map(Channel::Albedo).ok_or_else(|| TexError::Graph("no out-albedo".to_string()))?;
    let size = albedo.size();
    let height = map(Channel::Height);
    let specular = map(Channel::Specular);
    let normals = match height {
        Some((block, image)) => Some(normals_from_height(image, normal_strength(recipe, block))),
        // A specular mask is stored in the normal map, so it needs one.
        None => specular.map(|_| vec![0xff80_80ff; size.texels()]),
    };
    Ok(Maps {
        width: size.width(),
        height: size.height(),
        albedo: packed_albedo(albedo),
        normals,
        heights: height.map(|h| first_channel(h.1)),
        specular: specular.map(|s| first_channel(s.1)),
    })
}

/// Makes a recipe's maps at `size`, with nothing kept beyond them.
pub fn bake(recipe: &Recipe, size: Size) -> Result<Maps, TexError> {
    let graph = graph::compile(recipe)?;
    let outcome = graph::run(recipe, &graph, size, RunOptions::default())?;
    pack(recipe, &graph, &outcome)
}

/// The estimated nanoseconds of [`bake`], its packing and the renderer's taking of
/// the maps included, or the refusal `bake` would give, with no pixel made. A
/// directory of materials adds this up.
pub fn bake_cost(recipe: &Recipe, size: Size) -> Result<u64, TexError> {
    let graph = graph::compile(recipe)?;
    graph::plan::check(recipe, &graph, size, &[])?;
    let maps = [Channel::Albedo, Channel::Height, Channel::Specular]
        .into_iter()
        .filter(|c| graph.output(*c).is_some())
        .count() as u64;
    let packing = maps * PACK_NS_PER_TEXEL * size.texels() as u64;
    Ok(graph::page_cost(recipe, &graph, size).saturating_add(packing))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::recipe::read;

    fn size(n: u32) -> Size {
        Size::new(n, n).expect("size")
    }

    fn recipe(body: &str) -> Recipe {
        read(format!("miniscent-recipe 1\nsize 64 64\n{body}").as_bytes()).expect("reads")
    }

    /// The normal of a texel with no scale for the size: the formula again, so
    /// this cannot catch a wrong formula, only a wrong scale or a slip in packing.
    fn unscaled_normal(image: &Image, strength: f32, x: i32, y: i32) -> [i32; 3] {
        let h = |x: i32, y: i32| f32::from(image.at(x, y)[0]) / 65535.0;
        let dx = (h(x + 1, y) - h(x - 1, y)) * 0.5 * strength;
        let dy = (h(x, y + 1) - h(x, y - 1)) * 0.5 * strength;
        let length = (dx * dx + dy * dy + 1.0).sqrt();
        let pack = |v: f32| ((v / length * 0.5 + 0.5) * 255.0 + 0.5) as i32;
        [pack(-dx), pack(dy), pack(1.0)]
    }

    #[test]
    fn at_128_pixels_strength_means_what_it_says() {
        let r = read(
            b"miniscent-recipe 1\nsize 128 128\nop 0 0 1 noise freq_x=8 freq_y=8\nop 0 1 1 out-albedo\nop 1 0 1 noise freq_x=8 freq_y=8\nop 1 1 1 out-height normal_strength=5\n",
        )
        .expect("reads");
        let g = graph::compile(&r).expect("compiles");
        let out = graph::run(&r, &g, r.size, RunOptions::default()).expect("runs");
        let maps = pack(&r, &g, &out).expect("packs");
        let height = out.image(3).expect("height");
        let normals = maps.normals.expect("normals");
        for y in 0..128 {
            for x in 0..128 {
                let n = normals[(y * 128 + x) as usize];
                let got = [
                    (n >> 16) as i32 & 255,
                    (n >> 8) as i32 & 255,
                    n as i32 & 255,
                ];
                let want = unscaled_normal(height, 5.0, x, y);
                for c in 0..3 {
                    assert!((got[c] - want[c]).abs() <= 1, "{x},{y}: {got:?} {want:?}");
                }
            }
        }
    }

    #[test]
    fn relief_does_not_change_with_size() {
        let body = "op 0 0 1 flat\nop 0 1 1 out-albedo\nop 1 0 1 noise freq_x=2 freq_y=2 octaves=1\nop 1 1 1 out-height normal_strength=8\n";
        let r = recipe(body);
        let tilt = |n: u32| {
            let maps = bake(&r, size(n)).expect("bakes");
            let normals = maps.normals.expect("normals");
            let total: f64 = normals
                .iter()
                .map(|p| f64::from(((p >> 16) & 255) as i32 - 128).abs())
                .sum();
            total / normals.len() as f64
        };
        let (draft, full) = (tilt(64), tilt(256));
        assert!((draft - full).abs() < 0.1 * full.max(1.0), "{draft} {full}");
    }

    #[test]
    fn a_flat_height_gives_the_flat_normal() {
        let r = recipe("op 0 0 1 flat\nop 0 1 1 out-albedo\nop 1 0 1 flat\nop 1 1 1 out-height\n");
        let maps = bake(&r, size(8)).expect("bakes");
        assert!(
            maps.normals
                .expect("normals")
                .iter()
                .all(|&n| n == 0xff80_80ff)
        );
    }

    #[test]
    fn a_page_with_only_albedo_has_only_albedo() {
        let r = recipe("op 0 0 1 flat color=1,0,0\nop 0 1 1 out-albedo\n");
        let maps = bake(&r, size(4)).expect("bakes");
        assert_eq!(maps.albedo, vec![0xffff_0000; 16]);
        assert_eq!(
            (maps.normals, maps.heights, maps.specular),
            (None, None, None)
        );
    }

    #[test]
    fn a_specular_map_brings_a_flat_normal_map_to_live_in() {
        let r =
            recipe("op 0 0 1 flat\nop 0 1 1 out-albedo\nop 1 0 1 flat\nop 1 1 1 out-specular\n");
        let maps = bake(&r, size(4)).expect("bakes");
        assert_eq!(maps.normals, Some(vec![0xff80_80ff; 16]));
        assert_eq!(maps.specular, Some(vec![255; 16]));
    }

    #[test]
    fn a_page_with_no_albedo_is_an_error() {
        let r = recipe("op 0 0 1 flat\nop 0 1 1 out-height\n");
        assert!(bake(&r, size(4)).is_err());
    }

    #[test]
    fn eight_bit_rounding_reaches_both_ends() {
        assert_eq!(byte(0), 0);
        assert_eq!(byte(65535), 255);
        assert_eq!(byte(32768), 128);
    }
}
