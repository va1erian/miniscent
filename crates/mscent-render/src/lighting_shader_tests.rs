use super::*;
use crate::camera::Camera;
use crate::framebuffer::Framebuffer;
use crate::raster::{Vertex, draw_polygon};
use crate::texel_light::{KNEE, ROLL, add_spec, soft_limit};
use crate::texture::load_texture;
use mscent_world::lightmap::{BakeConfig, LightFace, PointLight, bake};
use mscent_world::{Brush, Op, compile};

const SIZE: u32 = 64;
const WHITE: u32 = 0xffff_ffff;

/// 64x64 target, 90 degree fov, camera at the origin looking down -Z.
pub(super) fn setup() -> (Framebuffer, Camera) {
    let cam = Camera::look_at(
        Vec3::ZERO,
        Vec3::new(0.0, 0.0, -1.0),
        Vec3::new(0.0, 1.0, 0.0),
        core::f32::consts::FRAC_PI_2,
        0.1,
        200.0,
        SIZE,
        SIZE,
    )
    .unwrap();
    (Framebuffer::new(SIZE, SIZE).unwrap(), cam)
}

/// A 4x4 wall at z = -2 facing the camera. Its screen footprint is exactly
/// the framebuffer, and one texture repeat spans the whole wall.
pub(super) fn wall() -> [Vertex; 4] {
    let p = |x: f32, y: f32, u: f32, v: f32| Vertex::new(Vec3::new(x, y, -2.0), u, v);
    [
        p(-2.0, 2.0, 0.0, 0.0),
        p(2.0, 2.0, 1.0, 0.0),
        p(2.0, -2.0, 1.0, 1.0),
        p(-2.0, -2.0, 0.0, 1.0),
    ]
}

/// Bake the wall lit by a lamp near its left edge: the right half is beyond
/// the lamp's radius and stays black (no ambient, no bounce).
pub(super) fn baked() -> Lightmap {
    // Space is solid by default; the room is carved out in front of the wall.
    let room = Brush::cuboid(
        mscent_math::Vec3d::new(-3.0, -3.0, -2.0),
        mscent_math::Vec3d::new(3.0, 3.0, 4.0),
        Op::Subtract,
        0,
    )
    .unwrap();
    let bsp = compile(&[room]).unwrap();
    let face = LightFace {
        vertices: wall().iter().map(|v| v.pos).collect(),
        normal: Vec3::new(0.0, 0.0, 1.0),
        density: Default::default(),
    };
    let lamp = PointLight {
        position: Vec3::new(-1.5, 0.0, 0.0),
        color: Vec3::new(32.0, 32.0, 32.0),
        radius: 3.0,
    };
    let cfg = BakeConfig {
        luxel_size: 0.25,
        workers: 2,
        ambient: Vec3::ZERO,
        albedo: 0.0,
        ..BakeConfig::default()
    };
    bake(&bsp, &[face], &[lamp], &cfg).unwrap()
}

pub(super) fn pack(n: Vec3) -> u32 {
    let c = |x: f32| ((x * 0.5 + 0.5) * 255.0).round() as u32;
    0xff00_0000 | c(n.x) << 16 | c(n.y) << 8 | c(n.z)
}

/// An 8x8 texture whose rows 0 and 4 are mortar grooves tilted by `tilt`
/// (a tangent-space xy offset), all other texels flat.
fn grooved(tilt: (f32, f32)) -> Texture {
    let flat = pack(Vec3::new(0.0, 0.0, 1.0));
    let groove = pack(Vec3::new(tilt.0, tilt.1, 0.7).normalized().unwrap());
    let normals = (0..64)
        .map(|i| if (i / 8) % 4 == 0 { groove } else { flat })
        .collect();
    Texture::new(8, 8, vec![WHITE; 64])
        .unwrap()
        .with_normals(normals)
        .unwrap()
}

/// Direction of the light at the luxel under pixel `(x, y)`, in the texture
/// frame the normal maps are authored in.
fn dir_at_pixel(lm: &Lightmap, tex: &Texture, x: u32, y: u32) -> Vec3 {
    let map = UvToLuxel::from_vertices(&lm.projections()[0], &wall(), tex).unwrap();
    let (u, v) = (x as f32 / 8.0, y as f32 / 8.0);
    let (lx, ly) = map.apply(u, v);
    map.to_texture_frame(lm.face_luxel(0, lx as u32, ly as u32).unwrap().dir)
}

pub(super) fn face<'a>(lm: &'a Lightmap, tex: &'a Texture, bump: bool) -> FaceLight<'a> {
    let map = UvToLuxel::from_vertices(&lm.projections()[0], &wall(), tex).unwrap();
    FaceLight {
        lightmap: lm,
        // Leaked: the tests are short-lived and the borrow outlives the helper.
        mips: Box::leak(Box::new(LightMips::build(lm))),
        face: 0,
        map,
        texture: tex,
        fog: None,
        bump,
    }
}

pub(super) fn render<S: SpanShader>(tex: &Texture, shader: &mut S) -> Vec<u32> {
    let (mut fb, cam) = setup();
    draw_polygon(&mut fb, &cam, &wall(), tex, shader);
    fb.pixels().to_vec()
}

pub(super) fn px(img: &[u32], x: u32, y: u32) -> u32 {
    img[(y * SIZE + x) as usize]
}

pub(super) fn red(c: u32) -> u32 {
    (c >> 16) & 0xff
}

#[test]
fn lit_side_is_brighter_than_unlit_side() {
    let lm = baked();
    let tex = grooved((0.0, 0.0));
    let mut sh = LightingShader::new(face(&lm, &tex, false)).unwrap();
    let img = render(&tex, &mut sh);
    let (lit, dark) = (px(&img, 8, 20), px(&img, 60, 20));
    assert!(red(lit) > 100, "lit side is {lit:#x}");
    assert_eq!(dark, 0xff00_0000, "beyond the lamp radius is black");
}

#[test]
fn light_falls_off_smoothly_across_spans() {
    let lm = baked();
    let tex = grooved((0.0, 0.0));
    let mut sh = LightingShader::new(face(&lm, &tex, false)).unwrap();
    let img = render(&tex, &mut sh);
    // The lightmap peaks under the lamp (pixel 8) and falls away from it,
    // with no jump at span boundaries (every 16 pixels). Sampling finer than
    // a span resolves the peak, so the row rises into it and falls after it.
    let row: Vec<u32> = (4..40).map(|x| red(px(&img, x, 20))).collect();
    let peak = row.iter().enumerate().max_by_key(|&(_, v)| *v).unwrap().0;
    assert!((2..8).contains(&peak), "peak at {peak} in {row:?}");
    assert!(row[..=peak].windows(2).all(|w| w[1] + 1 >= w[0]), "{row:?}");
    assert!(row[peak..].windows(2).all(|w| w[1] <= w[0] + 1), "{row:?}");
    let max_step = row.windows(2).map(|w| w[0].abs_diff(w[1])).max().unwrap();
    assert!(max_step <= 12, "step {max_step} in {row:?}");
}

#[test]
fn bumped_mortar_is_darker_and_bricks_match_flat() {
    let lm = baked();
    // Tilt mortar normals away from the light's tangent-plane direction.
    let d = dir_at_pixel(&lm, &grooved((0.0, 0.0)), 24, 36);
    let tex = grooved((-d.x, -d.y));
    let flat_img = render(
        &tex,
        &mut LightingShader::new(face(&lm, &tex, false)).unwrap(),
    );
    let bump_img = render(
        &tex,
        &mut LightingShader::new(face(&lm, &tex, true)).unwrap(),
    );
    // Texel row 4 covers pixel rows 32..40: mortar. Row 5 covers 40..48: brick.
    let (mortar_flat, mortar_bump) = (px(&flat_img, 24, 36), px(&bump_img, 24, 36));
    assert!(
        red(mortar_bump) + 10 < red(mortar_flat),
        "mortar flat {mortar_flat:#x} bumped {mortar_bump:#x}"
    );
    let (brick_flat, brick_bump) = (red(px(&flat_img, 24, 44)), red(px(&bump_img, 24, 44)));
    assert!(
        brick_flat.abs_diff(brick_bump) <= 3,
        "{brick_flat} vs {brick_bump}"
    );
}

#[test]
fn tilting_toward_the_light_brightens_the_groove() {
    let lm = baked();
    let d = dir_at_pixel(&lm, &grooved((0.0, 0.0)), 24, 36);
    let tex = grooved((d.x, d.y));
    let flat_img = render(
        &tex,
        &mut LightingShader::new(face(&lm, &tex, false)).unwrap(),
    );
    let bump_img = render(
        &tex,
        &mut LightingShader::new(face(&lm, &tex, true)).unwrap(),
    );
    assert!(red(px(&bump_img, 24, 36)) > red(px(&flat_img, 24, 36)) + 10);
}

#[test]
fn bump_flag_without_a_normal_map_changes_nothing() {
    let lm = baked();
    let tex = Texture::new(8, 8, vec![WHITE; 64]).unwrap();
    let off = render(
        &tex,
        &mut LightingShader::new(face(&lm, &tex, false)).unwrap(),
    );
    let on = render(
        &tex,
        &mut LightingShader::new(face(&lm, &tex, true)).unwrap(),
    );
    assert_eq!(off, on);
}

#[test]
fn zero_gloss_specular_matches_the_diffuse_path_exactly() {
    let lm = baked();
    let tex = grooved((0.3, 0.1));
    let table = SpecTable::new(0);
    let eye = Vec3::ZERO;
    for bump in [false, true] {
        let off = render(
            &tex,
            &mut LightingShader::new(face(&lm, &tex, bump)).unwrap(),
        );
        let mut sh = LightingShader::with_specular(face(&lm, &tex, bump), eye, &table).unwrap();
        assert_eq!(off, render(&tex, &mut sh), "bump = {bump}");
    }
}

#[test]
fn glossy_specular_adds_a_highlight_and_never_darkens() {
    let lm = baked();
    let tex = Texture::new(8, 8, vec![0xff60_6060; 64]).unwrap();
    let table = SpecTable::new(255);
    let off = render(
        &tex,
        &mut LightingShader::new(face(&lm, &tex, false)).unwrap(),
    );
    let mut sh = LightingShader::with_specular(face(&lm, &tex, false), Vec3::ZERO, &table).unwrap();
    let on = render(&tex, &mut sh);
    assert!(off.iter().zip(&on).all(|(a, b)| red(*b) >= red(*a)));
    // Mirror point of lamp (-1.5, 0, 0) and eye (0, 0, 0) in the wall plane
    // is x = -0.75, which projects to pixel column 20.
    let (peak_off, peak_on) = (red(px(&off, 20, 32)), red(px(&on, 20, 32)));
    assert!(peak_on > peak_off + 40, "peak {peak_off} -> {peak_on}");
    // Away from the mirror point the highlight fades: its lobe is broad
    // now, so compare the gain against the peak's instead of expecting zero.
    let far = red(px(&on, 4, 32)).saturating_sub(red(px(&off, 4, 32)));
    assert!(far + 30 < peak_on - peak_off, "far gain {far}");
}

#[test]
fn uv_map_covers_the_luxel_grid() {
    let lm = baked();
    let tex = grooved((0.0, 0.0));
    let proj = &lm.projections()[0];
    let map = UvToLuxel::from_vertices(proj, &wall(), &tex).unwrap();
    let corners = [(0.0, 0.0), (8.0, 0.0), (8.0, 8.0), (0.0, 8.0)];
    let mapped: Vec<(f32, f32)> = corners.iter().map(|&(u, v)| map.apply(u, v)).collect();
    let (w, h) = (proj.width as f32, proj.height as f32);
    for (x, y) in mapped {
        assert!(x.abs() < 1e-3 || (x - w).abs() < 1e-3, "x = {x}");
        assert!(y.abs() < 1e-3 || (y - h).abs() < 1e-3, "y = {y}");
    }
}

#[test]
fn collinear_uv_is_refused_and_missing_face_is_none() {
    let lm = baked();
    let tex = grooved((0.0, 0.0));
    let line = [
        Vertex::new(Vec3::ZERO, 0.0, 0.0),
        Vertex::new(Vec3::new(1.0, 0.0, 0.0), 0.5, 0.5),
        Vertex::new(Vec3::new(2.0, 0.0, 0.0), 1.0, 1.0),
    ];
    assert!(UvToLuxel::from_vertices(&lm.projections()[0], &line, &tex).is_none());
    let mut f = face(&lm, &tex, false);
    f.face = 7;
    assert!(LightingShader::new(f).is_none());
}

#[test]
fn fog_blends_toward_its_colour() {
    let lm = baked();
    let tex = grooved((0.0, 0.0));
    let mut f = face(&lm, &tex, false);
    f.fog = Fog::new(0xff00_00ff, 0.0, 4.0);
    let img = render(&tex, &mut LightingShader::new(f).unwrap());
    // Wall is 2 units away: half fogged, so blue is lifted on the dark side.
    assert!(px(&img, 60, 20) & 0xff > 100);
}

/// Every `k`th texel of albedo and normals: a smaller texture with the same
/// look, so the wall shows it at about half a texel per pixel (magnified)
/// instead of two.
fn decimated(t: &Texture, k: u32) -> Texture {
    let (w, h) = (t.width() / k, t.height() / k);
    let at = |f: &dyn Fn(i32, i32) -> u32| -> Vec<u32> {
        (0..w * h)
            .map(|i| f((i % w * k) as i32, (i / w * k) as i32))
            .collect()
    };
    let albedo = at(&|x, y| t.texel(x, y));
    let normals = at(&|x, y| t.normal_texel(x, y).unwrap());
    Texture::new(w, h, albedo)
        .unwrap()
        .with_normals(normals)
        .unwrap()
}

#[test]
fn bump_fades_out_where_the_normal_map_is_minified() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/textures");
    let tex = load_texture(&dir, "brick").unwrap();
    let lm = baked();
    // Two repeats across the wall: four texels per pixel, far past aliasing.
    let dense: Vec<Vertex> = wall()
        .iter()
        .map(|v| Vertex::new(v.pos, v.uv.x * 2.0, v.uv.y * 2.0))
        .collect();
    let map = UvToLuxel::from_vertices(&lm.projections()[0], &dense, &tex).unwrap();
    let draw = |bump: bool| {
        let (mut fb, cam) = setup();
        let fl = FaceLight {
            map,
            ..face(&lm, &tex, bump)
        };
        draw_polygon(
            &mut fb,
            &cam,
            &dense,
            &tex,
            &mut LightingShader::new(fl).unwrap(),
        );
        fb.pixels().to_vec()
    };
    // The bumped picture equals the flat one: dither included, no aliased relief.
    assert_eq!(draw(true), draw(false), "aliased bumps still show");
}

#[test]
fn real_brick_bump_changes_exactly_the_non_flat_texels() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/textures");
    let tex = decimated(&load_texture(&dir, "brick").unwrap(), 4);
    let lm = baked();
    let flat = render(
        &tex,
        &mut LightingShader::new(face(&lm, &tex, false)).unwrap(),
    );
    let bump = render(
        &tex,
        &mut LightingShader::new(face(&lm, &tex, true)).unwrap(),
    );
    let (mut tilted, mut changed, mut flat_off) = (0, 0, 0);
    for y in 0..SIZE {
        for x in 0..SIZE {
            let (a, b) = (red(px(&flat, x, y)), red(px(&bump, x, y)));
            if a < 40 {
                continue; // too dark to compare
            }
            let centre = |p: u32, n: u32| ((p as f32 + 0.5) * n as f32 / SIZE as f32) as i32;
            let (tx, ty) = (centre(x, tex.width()), centre(y, tex.height()));
            let n = tex.normal(tx, ty).unwrap();
            if n.z > 0.9999 {
                // Relief scaling lets near-flat texels differ a little.
                flat_off += u32::from(a.abs_diff(b) * 6 > a);
            } else {
                tilted += 1;
                changed += u32::from(a != b);
            }
        }
    }
    assert!(tilted > 50, "test needs tilted lit texels, got {tilted}");
    assert!(
        changed * 10 > tilted * 7,
        "{changed} of {tilted} tilted pixels changed"
    );
    assert_eq!(flat_off, 0, "flat texels must match the unbumped render");
}

#[test]
fn huge_light_colour_saturates_instead_of_overflowing() {
    // A float-to-int cast saturates at u32::MAX, so adding it to a channel
    // must not wrap (debug builds panic on the overflow).
    let k = Vec3::new(1.0e12, 1.0e12, 1.0e12);
    assert_eq!(add_spec(0xff80_8080, k, 1.0), 0xffff_ffff);
    assert_eq!(mul_rgb3(0xff80_8080, k, 0.0), 0xffff_ffff);
    let nan = Vec3::new(f32::NAN, f32::INFINITY, -1.0);
    let _ = (
        add_spec(0xff80_8080, nan, f32::NAN),
        mul_rgb3(0xff80_8080, nan, 0.0),
    );
}

#[test]
fn soft_limit_is_monotonic_continuous_and_bounded() {
    assert_eq!(soft_limit(100.0), 100.0);
    let mut prev = 0.0;
    for i in 0..4000 {
        let y = soft_limit(i as f32);
        assert!(y >= prev && y < KNEE + ROLL, "{i} -> {y}");
        prev = y;
    }
    // Two very bright inputs stay distinguishable (no flat clipping).
    assert!(soft_limit(900.0) > soft_limit(500.0) + 5.0);
}

/// A texture whose every texel has the normal `tilt` (texture-space x along
/// +u, y towards the top of the image, as the generated maps are authored).
fn uniformly_tilted(tilt: (f32, f32)) -> Texture {
    let n = pack(Vec3::new(tilt.0, tilt.1, 1.0).normalized().unwrap());
    Texture::new(8, 8, vec![WHITE; 64])
        .unwrap()
        .with_normals(vec![n; 64])
        .unwrap()
}

fn bumped_red(tilt: (f32, f32), x: u32, y: u32) -> u32 {
    let lm = baked();
    let tex = uniformly_tilted(tilt);
    let img = render(
        &tex,
        &mut LightingShader::new(face(&lm, &tex, true)).unwrap(),
    );
    red(px(&img, x, y))
}

#[test]
fn bump_light_direction_follows_the_texture_axes_not_the_lightmap_axes() {
    // The lamp is at world x = -1.5, y = 0: left of pixel column 24 and level
    // with pixel row 32. Normals tilted towards -u (left) face the lamp.
    let (toward, away, flat) = (
        bumped_red((-0.6, 0.0), 24, 32),
        bumped_red((0.6, 0.0), 24, 32),
        bumped_red((0.0, 0.0), 24, 32),
    );
    assert!(toward > flat + 8, "toward {toward} flat {flat}");
    assert!(away + 8 < flat, "away {away} flat {flat}");
}

#[test]
fn bump_light_direction_has_the_right_vertical_sense() {
    // Pixel row 44 is below the lamp (y = -0.75 vs 0): the lamp is towards the
    // top of the image, which is +y in the normal map.
    let (up, down, flat) = (
        bumped_red((0.0, 0.6), 16, 44),
        bumped_red((0.0, -0.6), 16, 44),
        bumped_red((0.0, 0.0), 16, 44),
    );
    assert!(up > flat + 8, "up {up} flat {flat}");
    assert!(down + 8 < flat, "down {down} flat {flat}");
}
