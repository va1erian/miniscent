//! [`shade_texel`] against the game's shader on a synthetic face: same texels,
//! same light, same bytes. The Texture Studio's lit view is only worth looking at
//! if this holds.

use mscent_math::Vec3;

use super::lighting_shader_tests::{baked, face, pack};
use super::*;
use crate::texel_light::{TexelLight, TexelSurface, shade_texel};

const SIZE: u32 = 64;
/// Pixels of a span, as the shader counts them.
const LEN: u32 = SPAN_LEN as u32;

/// Tilted normals everywhere, a checkered specular mask, and a colour per texel.
fn texture() -> Texture {
    let len = (SIZE * SIZE) as usize;
    let at = |i: usize| ((i as u32 % SIZE), (i as u32 / SIZE));
    let albedo = (0..len)
        .map(|i| {
            let (x, y) = at(i);
            0xff00_0000 | (x * 4) << 16 | (y * 4) << 8 | 0x60
        })
        .collect();
    let normals = (0..len)
        .map(|i| {
            let (x, y) = at(i);
            let tilt = |v: u32| (v as f32 / SIZE as f32 - 0.5) * 0.8;
            pack(Vec3::new(tilt(x), tilt(y), 1.0).normalized().unwrap())
        })
        .collect();
    let mask: Vec<u8> = (0..len)
        .map(|i| {
            let (x, y) = at(i);
            if (x / 8 + y / 8) % 2 == 0 { 255 } else { 96 }
        })
        .collect();
    Texture::new(SIZE, SIZE, albedo)
        .unwrap()
        .with_normals(normals)
        .unwrap()
        .with_specular(&mask)
        .unwrap()
}

/// A span along texture row `v`, one pixel per texel.
fn span(v: f32) -> (SpanEnd, SpanEnd) {
    let end = |x: i32| SpanEnd {
        x,
        y: v as i32,
        u: x as f32 + 0.5,
        v: v + 0.5,
        depth: 2.0,
    };
    (end(0), end(SPAN_LEN))
}

#[test]
fn the_shared_function_gives_the_shaders_bytes_on_a_synthetic_face() {
    let lm = baked();
    let tex = texture();
    let table = SpecTable::new(255);
    let eye = Vec3::new(-1.0, 0.0, 0.5);
    let mut shader = LightingShader::with_specular(face(&lm, &tex, true), eye, &table).unwrap();
    shader.set_texel_rate(1.0);
    let map = tex.normal_view().unwrap();
    let (mut compared, mut lit, mut highlighted) = (0, 0, 0);
    for row in 0..SIZE {
        let (start, end) = span(row as f32);
        shader.begin_span(&start, &end, LEN);
        for i in 0..LEN {
            let (x, y) = (i as i32, row as i32);
            let (fu, fv) = ((x << 16) + 0x8000, (y << 16) + 0x8000);
            let texel = tex.texel(x, y);
            let got = shader.shade(i, texel, fu, fv);
            // The shader's own state for this pixel, and no other of its logic.
            let c = shader.col[i as usize];
            let seg = &shader.segs[(i >> shader.shift) as usize];
            let f = (i & ((1 << shader.shift) - 1)) as f32;
            let sample = NormalSample::fetch(&map, fu, fv);
            let light = TexelLight {
                color: Vec3::new(c[0], c[1], c[2]),
                dir: seg.dir0 + seg.ddir * f,
                half: seg.half0 + seg.dhalf * f,
            };
            // The shader fades the mask to the mean; at full bump that fade is
            // the mask itself, to within a rounding this keeps the same.
            let mean = tex.spec_mean();
            let surface = TexelSurface {
                normal: Some(sample.normal()),
                table: Some(&table),
                mask: mean + (sample.mask() - mean),
            };
            let dither = crate::texel_light::dither_of(x, y);
            assert_eq!(
                shade_texel(texel, light, surface, dither),
                got,
                "pixel {x},{y}"
            );
            let plain = TexelSurface {
                table: None,
                ..surface
            };
            compared += 1;
            lit += i32::from(got & 0x00ff_ffff != 0);
            highlighted += i32::from(shade_texel(texel, light, plain, dither) != got);
        }
    }
    assert_eq!(compared, (SIZE * LEN) as i32);
    // Pixels that are black or have no highlight would prove little.
    assert!(lit > compared / 8, "{lit} lit of {compared}");
    assert!(highlighted > compared / 16, "{highlighted} highlighted");
}

#[test]
fn with_nothing_but_light_it_is_the_albedo_under_the_light() {
    let light = TexelLight {
        color: Vec3::new(128.0, 128.0, 128.0),
        dir: Vec3::new(0.0, 0.0, 1.0),
        half: Vec3::new(0.0, 0.0, 1.0),
    };
    let surface = TexelSurface {
        normal: None,
        table: None,
        mask: 1.0,
    };
    assert_eq!(shade_texel(0xff80_4020, light, surface, 0.0), 0xff40_2010);
}

#[test]
fn a_mask_of_zero_takes_the_highlight_away() {
    let table = SpecTable::new(255);
    let light = TexelLight {
        color: Vec3::new(128.0, 128.0, 128.0),
        dir: Vec3::new(0.0, 0.0, 1.0),
        half: Vec3::new(0.0, 0.0, 1.0),
    };
    let surface = |mask| TexelSurface {
        normal: None,
        table: Some(&table),
        mask,
    };
    let (full, none) = (
        shade_texel(0xff80_4020, light, surface(1.0), 0.0),
        shade_texel(0xff80_4020, light, surface(0.0), 0.0),
    );
    assert_eq!(none, 0xff40_2010);
    assert!(full & 0xff > none & 0xff, "{full:x} {none:x}");
}
