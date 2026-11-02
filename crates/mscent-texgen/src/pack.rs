//! Finished maps as the renderer stores them: `0xAARRGGBB` albedo, a tangent-space
//! normal map derived from the height, and 8-bit height, specular and alpha.

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
    /// Coverage per texel; `None` when the recipe has no `out-alpha`. The colour
    /// under a clear texel is already bled (see [`bleed`]).
    pub alpha: Option<Vec<u8>>,
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

/// Gives every clear texel (alpha 0) the colour of the nearest texel that is not,
/// so that mips and bilinear taps at a cutout's edge blend the colour of the shape
/// and not whatever the recipe left underneath, which would fringe. Done here and
/// not as an operator because it needs the albedo and the alpha together, a
/// recipe that forgot it would fringe silently, and it is wanted every time. The
/// spread is breadth-first over wrapped 4-neighbours in a fixed order, so it is
/// linear in the texels and the same on every machine. A map with nothing opaque
/// is left as it is.
fn bleed(albedo: &mut [u32], alpha: &[u8], size: Size) {
    let (w, h) = (size.width() as usize, size.height() as usize);
    let mut done: Vec<bool> = alpha.iter().map(|&a| a > 0).collect();
    let mut queue: Vec<usize> = (0..done.len()).filter(|&i| done[i]).collect();
    let mut head = 0;
    while let Some(&i) = queue.get(head) {
        head += 1;
        let (x, y) = (i % w, i / w);
        let around = [
            y * w + (x + 1) % w,
            y * w + (x + w - 1) % w,
            (y + 1) % h * w + x,
            (y + h - 1) % h * w + x,
        ];
        for n in around {
            if !done[n] {
                done[n] = true;
                albedo[n] = albedo[i];
                queue.push(n);
            }
        }
    }
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
    let alpha = map(Channel::Alpha);
    let normals = match height {
        Some((block, image)) => Some(normals_from_height(image, normal_strength(recipe, block))),
        // A specular mask is stored in the normal map, so it needs one.
        None => specular.map(|_| vec![0xff80_80ff; size.texels()]),
    };
    let mut texels = packed_albedo(albedo);
    let alpha = alpha.map(|a| first_channel(a.1));
    if let Some(alpha) = &alpha {
        bleed(&mut texels, alpha, size);
    }
    Ok(Maps {
        width: size.width(),
        height: size.height(),
        albedo: texels,
        normals,
        heights: height.map(|h| first_channel(h.1)),
        specular: specular.map(|s| first_channel(s.1)),
        alpha,
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
    // The alpha map counts for two: its own, and the bleed over the albedo.
    let maps = [Channel::Albedo, Channel::Height, Channel::Specular]
        .into_iter()
        .filter(|c| graph.output(*c).is_some())
        .count() as u64
        + 2 * u64::from(graph.output(Channel::Alpha).is_some());
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
    fn the_alpha_map_is_the_red_of_its_page_and_other_pages_have_none() {
        let r = recipe(
            "op 0 0 1 flat color=1,0,0
op 0 1 1 out-albedo
op 1 0 1 flat color=0.5,0,0
op 1 1 1 out-alpha
",
        );
        let maps = bake(&r, size(4)).expect("bakes");
        assert_eq!(maps.alpha, Some(vec![128; 16]));
        let plain = recipe(
            "op 0 0 1 flat
op 0 1 1 out-albedo
",
        );
        assert_eq!(bake(&plain, size(4)).expect("bakes").alpha, None);
    }

    #[test]
    fn colour_under_clear_texels_is_bled_from_the_nearest_and_across_the_seam() {
        // A red dot at the left edge on blue: the clear texels read red or blue as
        // they are near, and the one beyond the seam takes the dot's colour.
        let r = recipe(
            "op 0 0 1 rect x=0 y=0.5 w=0.25 h=0.25 color=1,0,0 background=0,0,1
op 0 1 1 out-albedo
op 1 0 1 rect x=0 y=0.5 w=0.25 h=0.25
op 1 1 1 out-alpha
",
        );
        let maps = bake(&r, size(16)).expect("bakes");
        let alpha = maps.alpha.expect("alpha");
        let at = |x: usize, y: usize| y * 16 + x;
        assert_eq!(alpha[at(0, 8)], 255);
        assert_eq!(alpha[at(8, 2)], 0);
        // Every texel is red now: the blue under the clear ones is gone.
        assert!(
            maps.albedo.iter().all(|&t| t == 0xffff_0000),
            "{:x?}",
            maps.albedo
        );
    }

    #[test]
    fn a_map_with_nothing_opaque_is_left_as_it_is() {
        let r = recipe(
            "op 0 0 1 flat color=0,1,0
op 0 1 1 out-albedo
op 1 0 1 flat color=0,0,0
op 1 1 1 out-alpha
",
        );
        let maps = bake(&r, size(4)).expect("bakes");
        assert_eq!(maps.albedo, vec![0xff00_ff00; 16]);
        assert_eq!(maps.alpha, Some(vec![0; 16]));
    }

    #[test]
    fn an_alpha_output_is_charged_for_its_map_and_its_bleed() {
        let plain = recipe(
            "op 0 0 1 flat
op 0 1 1 out-albedo
",
        );
        let masked = recipe(
            "op 0 0 1 flat
op 0 1 1 out-albedo
op 1 0 1 flat
op 1 1 1 out-alpha
",
        );
        let n = size(64);
        let (a, b) = (
            bake_cost(&plain, n).expect("cost"),
            bake_cost(&masked, n).expect("cost"),
        );
        // A flat is 3 ns a texel, and the alpha is charged as two maps: its own and the bleed.
        assert_eq!(b - a, (3 + 2 * PACK_NS_PER_TEXEL) * n.texels() as u64);
    }

    #[test]
    fn a_page_with_only_albedo_has_only_albedo() {
        let r = recipe("op 0 0 1 flat color=1,0,0\nop 0 1 1 out-albedo\n");
        let maps = bake(&r, size(4)).expect("bakes");
        assert_eq!(maps.albedo, vec![0xffff_0000; 16]);
        assert_eq!(
            (maps.normals, maps.heights, maps.specular, maps.alpha),
            (None, None, None, None)
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
