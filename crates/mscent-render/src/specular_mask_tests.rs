//! The specular mask, read from real pixels: where it may be fetched, where it
//! must fall back to the mean, and that a full mask changes nothing.

use super::lighting_shader_tests::{baked, face, pack, px, red, render, setup, wall};
use super::*;
use crate::raster::{Vertex, draw_polygon};

/// Grey enough that a highlight has room to add.
const GREY: u32 = 0xff60_6060;

fn flat_normals(side: u32) -> Texture {
    let len = (side * side) as usize;
    Texture::new(side, side, vec![GREY; len])
        .unwrap()
        .with_normals(vec![pack(FLAT_NORMAL); len])
        .unwrap()
}

/// `side` x `side` with the mask off in rows `off` of every 8, and full in the
/// others (so the two share a mean that a uniform mask can match).
fn banded(side: u32, off: &[u32]) -> Texture {
    let mask: Vec<u8> = (0..side * side)
        .map(|i| {
            if off.contains(&((i / side) % 8)) {
                0
            } else {
                255
            }
        })
        .collect();
    flat_normals(side).with_specular(&mask).unwrap()
}

/// The same texture with every texel at its mean: what the bands must average
/// to wherever the mask is not read.
fn uniform_as(tex: &Texture) -> Texture {
    let mean = (tex.spec_mean() * 255.0).round() as u8;
    let len = (tex.width() * tex.height()) as usize;
    flat_normals(tex.width())
        .with_specular(&vec![mean; len])
        .unwrap()
}

/// Spec and diffuse pictures of `tex` on the wall, with the UVs scaled by
/// `repeats` so the texel rate can be chosen.
fn pictures(tex: &Texture, repeats: f32, bump: bool) -> (Vec<u32>, Vec<u32>) {
    let lm = Box::leak(Box::new(baked()));
    let table = Box::leak(Box::new(SpecTable::new(255)));
    let verts: Vec<Vertex> = wall()
        .iter()
        .map(|v| Vertex::new(v.pos, v.uv.x * repeats, v.uv.y * repeats))
        .collect();
    let map = UvToLuxel::from_vertices(&lm.projections()[0], &verts, tex).unwrap();
    let light = FaceLight {
        map,
        ..face(lm, tex, bump)
    };
    let (mut lit, cam) = setup();
    let mut sh = LightingShader::with_specular(light, Vec3::ZERO, table).unwrap();
    draw_polygon(&mut lit, &cam, &verts, tex, &mut sh);
    let (mut plain, cam) = setup();
    let mut sh = LightingShader::new(light).unwrap();
    draw_polygon(&mut plain, &cam, &verts, tex, &mut sh);
    (lit.pixels().to_vec(), plain.pixels().to_vec())
}

#[test]
fn a_masked_highlight_differs_where_the_mask_differs() {
    // 8 texels over 64 pixels: texel rows 2 and 3 are pixels 16..32. Pixel rows
    // 26 and 37 are equally far either side of the lamp's row 32 (rows 31 and 32
    // meet at it), so with no mask they would get about the same highlight, and
    // the bilinear fetch at each reads only one kind of texel.
    let tex = banded(8, &[2, 3]);
    let (on, off) = pictures(&tex, 1.0, true);
    let (masked, full) = (26, 37);
    assert_eq!(
        px(&on, 20, masked),
        px(&off, 20, masked),
        "a zero mask adds nothing at all"
    );
    let gain = red(px(&on, 20, full)) - red(px(&off, 20, full));
    assert!(gain > 30, "the unmasked row keeps its highlight: {gain}");

    let unmasked = flat_normals(8);
    let (bare, _) = pictures(&unmasked, 1.0, true);
    assert_eq!(
        px(&on, 20, full),
        px(&bare, 20, full),
        "a full texel is the same as no mask"
    );
    assert!(
        red(px(&bare, 20, masked)) > red(px(&off, 20, masked)) + 30,
        "without the mask that row has a highlight too"
    );
}

#[test]
fn the_mask_never_darkens_the_diffuse_picture() {
    let tex = banded(8, &[1, 3, 5, 7]);
    let (on, off) = pictures(&tex, 1.0, true);
    assert!(on.iter().zip(&off).all(|(a, b)| red(*a) >= red(*b)));
}

#[test]
fn a_full_mask_matches_no_mask_in_every_pixel() {
    let plain = flat_normals(8);
    let full = flat_normals(8).with_specular(&[255; 64]).unwrap();
    assert_eq!(pictures(&plain, 1.0, true), pictures(&full, 1.0, true));
    assert_eq!(pictures(&plain, 1.0, false), pictures(&full, 1.0, false));
}

#[test]
fn a_masked_texture_draws_the_same_diffuse_picture() {
    let (_, masked) = pictures(&banded(8, &[2, 3]), 1.0, true);
    let (_, plain) = pictures(&flat_normals(8), 1.0, true);
    assert_eq!(masked, plain, "only the highlight reads the alpha");
}

#[test]
fn past_the_bump_fade_the_mean_stands_in_for_the_mask() {
    // Two repeats of 128 texels over 64 pixels: four texels per pixel, so the
    // normal fetch is skipped and the alpha is out of reach.
    let tex = banded(128, &[0, 1, 2, 3]);
    let uniform = uniform_as(&tex);
    assert!(tex.spec_mean() > 0.4 && tex.spec_mean() < 0.6);
    assert_eq!(pictures(&tex, 2.0, true), pictures(&uniform, 2.0, true));
    // It is a mean and not "no mask" or "all masked".
    let (on, off) = pictures(&tex, 2.0, true);
    let (bare, _) = pictures(&flat_normals(128), 2.0, true);
    let gain = |a: &[u32]| red(px(a, 20, 32)) - red(px(&off, 20, 32));
    assert!(gain(&on) > 0, "the mean still shows");
    assert!(gain(&on) < gain(&bare), "but below the full highlight");
}

#[test]
fn with_bump_mapping_off_the_mean_stands_in_for_the_mask() {
    let tex = banded(8, &[0, 1, 2, 3]);
    let uniform = uniform_as(&tex);
    assert_eq!(pictures(&tex, 1.0, false), pictures(&uniform, 1.0, false));
}

#[test]
fn the_mask_fades_towards_the_mean_while_the_map_is_minified() {
    // One repeat of 128 texels over 64 pixels: two texels per pixel, half way
    // through the fade. Bands 8 texels wide make the pixel rows easy to read.
    let tex = banded(128, &[0, 1, 2, 3]);
    let (on, off) = pictures(&tex, 1.0, true);
    let (sharp, sharp_off) = pictures(&banded(16, &[0, 1, 2, 3]), 1.0, true);
    // Row 28 of the screen is texel row 56, band 7 and full; row 36 is texel
    // row 72, band 1 and off. Each sits nearer the mean than in the unfaded map.
    let gain = |a: &[u32], b: &[u32], y: u32| red(px(a, 20, y)) - red(px(b, 20, y));
    assert!(gain(&on, &off, 36) > 0, "off, but faded up from zero");
    assert!(
        gain(&on, &off, 28) > gain(&on, &off, 36),
        "bands still show"
    );
    assert!(
        gain(&on, &off, 28) < gain(&sharp, &sharp_off, 28),
        "full, but faded down"
    );
}

#[test]
fn a_texture_with_no_map_at_all_has_an_unchanged_highlight() {
    // On black the picture is the highlight alone, and half grey shows the light
    // level. At the mirror point of lamp and eye the normal faces the half vector,
    // so the highlight must be the table's peak of that light, with no mask in it.
    let lm = baked();
    let table = SpecTable::new(128);
    let black = Texture::new(8, 8, vec![0xff00_0000; 64]).unwrap();
    assert_eq!(black.spec_mean(), 1.0);
    let f = face(&lm, &black, true);
    let mut sh = LightingShader::with_specular(f, Vec3::ZERO, &table).unwrap();
    let highlight = red(px(&render(&black, &mut sh), 20, 32)) as f32;
    let half = Texture::new(8, 8, vec![0xff80_8080; 64]).unwrap();
    let f = face(&lm, &half, true);
    let img = render(&half, &mut LightingShader::new(f).unwrap());
    let light = red(px(&img, 20, 32)) as f32 * 2.0;
    let want = light * table.response(1.0) * SPEC_GAIN;
    assert!(want > 50.0, "a highlight worth measuring: {want}");
    assert!(
        (highlight - want).abs() < want * 0.1,
        "highlight {highlight}, a full one is {want}"
    );
}
