//! Cutout textures: the stock sky materials, read as pixels. The planet is a hard
//! disc on a plate, the clouds a soft one, both in a vista in front of the stars.

use std::path::{Path, PathBuf};

use mscent_math::rng::Rng;
use mscent_math::{Vec3, Vec3d};
use mscent_render::{
    BuildConfig, Framebuffer, LevelRenderer, Material, MaterialId, MaterialTable, Quality, Texture,
    TextureError, View, load_texture,
};
use mscent_world::materials::{SKY_CLOUDS, SKY_PLANET, SKY_RIDGE, SKY_STARS, VIEW_FIRST};
use mscent_world::{Brush, Entity, Level, MAX_VISTA_FOG, Op};

const W: u32 = 160;
const H: u32 = 90;
/// Where the stars' room is, and (twice it) the room with the clouds.
const AWAY: f64 = 1000.0;
/// The plate: how far in front of the eye, and how wide.
const PLATE_Y: f64 = 24.0;
const PLATE: f64 = 12.0;

fn dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/textures")
}

fn stock() -> MaterialTable {
    MaterialTable::load_dir(&dir()).expect("textures")
}

fn vd(x: f64, y: f64, z: f64) -> Vec3d {
    Vec3d::new(x, y, z)
}

fn cuboid(min: Vec3d, max: Vec3d, op: Op, order: u32, material: u16) -> Brush {
    Brush::cuboid(min, max, op, order)
        .expect("a cuboid")
        .with_material(material)
}

fn room_at(x: f64, order: u32, material: u16) -> Brush {
    cuboid(
        vd(x - 16.0, -16.0, -16.0),
        vd(x + 16.0, 48.0, 16.0),
        Op::Subtract,
        order,
        material,
    )
}

/// A thin square plate seen from the eye, its near face mapped once across.
fn plate_at(x: f64, order: u32, material: u16, side: u32) -> Brush {
    let half = PLATE / 2.0;
    let mut b = cuboid(
        vd(x - half, PLATE_Y, -half),
        vd(x + half, PLATE_Y + 1.0, half),
        Op::Add,
        order,
        material,
    );
    let face = b.plane_facing(vd(0.0, -1.0, 0.0)).expect("the near face");
    let mut t = b.texinfos()[face];
    let scale = PLATE as f32 / side as f32;
    t.u_axis = Vec3::new(1.0, 0.0, 0.0);
    t.v_axis = Vec3::new(0.0, 0.0, -1.0);
    t.rotation_deg = 0.0;
    t.set_scale(scale, scale);
    t.u_shift = -(x - half) as f32 / scale;
    t.v_shift = half as f32 / scale;
    assert!(b.set_texinfo(face, t));
    b
}

fn layer(x: f32, layer: u8) -> Entity {
    Entity::VistaLayer {
        pos: Vec3::new(x, 0.0, 0.0),
        centre: Vec3::ZERO,
        slot: 0,
        layer,
        parallax: 0.0,
        spin: Vec3::ZERO,
        pan: [0.0, 0.0],
        fog_start: 0.0,
        fog_end: MAX_VISTA_FOG,
        fog_color: 0,
    }
}

/// The play room is all viewport onto slot 0, whose layer 0 is a star room, with a
/// plate in it when `plate` says so.
fn level(plate: Option<(u16, u32)>) -> Level {
    let mut brushes = vec![room_at(0.0, 0, VIEW_FIRST), room_at(AWAY, 1, SKY_STARS)];
    if let Some((material, side)) = plate {
        brushes.push(plate_at(AWAY, 2, material, side));
    }
    Level {
        brushes,
        entities: vec![layer(AWAY as f32, 0)],
        ..Level::default()
    }
}

fn picture(level: &Level, table: MaterialTable) -> Framebuffer {
    let r = LevelRenderer::from_level(level, table, &BuildConfig::default()).expect("builds");
    let mut fb = Framebuffer::new(W, H).expect("fb");
    let view = View::from_angles(Vec3::ZERO, 0.0, 0.0, 0.0);
    r.render(&mut fb, &view, Quality::Low, &[], None)
        .expect("renders");
    fb
}

fn px(fb: &Framebuffer, x: i32, y: i32) -> u32 {
    fb.pixel(x, y).expect("in range")
}

fn every_pixel() -> impl Iterator<Item = (i32, i32)> {
    (0..H as i32).flat_map(|y| (0..W as i32).map(move |x| (x, y)))
}

/// Bounding box `(x0, y0, x1, y1)` of the pixels that differ between two pictures.
fn changed(a: &Framebuffer, b: &Framebuffer) -> Option<(i32, i32, i32, i32)> {
    let mut bb: Option<(i32, i32, i32, i32)> = None;
    for (x, y) in every_pixel().filter(|&(x, y)| px(a, x, y) != px(b, x, y)) {
        let (x0, y0, x1, y1) = bb.unwrap_or((x, y, x, y));
        bb = Some((x0.min(x), y0.min(y), x1.max(x), y1.max(y)));
    }
    bb
}

fn channels(c: u32) -> [i32; 3] {
    [
        (c >> 16 & 255) as i32,
        (c >> 8 & 255) as i32,
        (c & 255) as i32,
    ]
}

#[test]
fn the_stock_sky_textures_load_and_only_the_stars_are_opaque() {
    let table = stock();
    for (id, masked, (w, h)) in [
        (SKY_STARS, false, (512, 512)),
        (SKY_CLOUDS, true, (512, 512)),
        (SKY_RIDGE, true, (512, 256)),
        (SKY_PLANET, true, (256, 256)),
    ] {
        let m = table.get(MaterialId(id)).expect("listed");
        let t = &m.texture;
        assert_eq!((t.width(), t.height()), (w, h), "{}", m.name);
        assert_eq!(t.is_masked(), masked, "{}", m.name);
        assert_eq!(m.gloss, 0, "{}", m.name);
    }
}

#[test]
fn an_opaque_texture_has_no_plane_and_the_older_textures_are_all_opaque() {
    for (id, m) in stock().iter().filter(|(id, _)| id.0 <= SKY_STARS) {
        assert!(!m.texture.is_masked(), "{} ({})", m.name, id.0);
        assert!(m.texture.alpha_sampler(0).is_none(), "{}", m.name);
    }
}

#[test]
fn coverage_survives_the_mips_of_every_cutout() {
    let table = stock().filtered();
    for id in [SKY_CLOUDS, SKY_RIDGE, SKY_PLANET] {
        let m = table.get(MaterialId(id)).expect("listed");
        let t = &m.texture;
        let mean = t
            .alpha_sampler(t.mip_count())
            .expect("masked")
            .nearest(0, 0);
        assert!((20..235).contains(&mean), "{}: top level is {mean}", m.name);
        // A level in the middle of the chain still has clear and solid areas.
        let mid = t.alpha_sampler(3).expect("masked");
        let seen: Vec<u8> = (0..32)
            .flat_map(|y| (0..32).map(move |x| (x, y)))
            .map(|(x, y)| mid.nearest(x * 16, y * 16))
            .collect();
        assert!(seen.iter().any(|&a| a < 8), "{} has clear", m.name);
        assert!(seen.iter().any(|&a| a > 120), "{} has cover", m.name);
    }
}

#[test]
fn colour_under_the_clear_texels_is_not_black() {
    // Bled colour keeps a mip or a bilinear tap across an edge from darkening it.
    let ridge = load_texture(&dir(), "sky-ridge").expect("loads");
    let (mut n, mut sum) = (0, 0);
    for (x, y) in (0..256).flat_map(|y| (0..512).map(move |x| (x, y))) {
        if ridge.alpha_at(x, y) == 0 {
            n += 1;
            sum += channels(ridge.texel(x, y)).iter().sum::<i32>();
        }
    }
    assert!(n > 1000, "some sky above the ridge: {n}");
    assert!(sum / n > 60, "mean {}", sum / n);
}

/// The stock table with material `id`'s texture rebuilt by `f`: the same plate
/// geometry can then be drawn solid, clear or as it is, and compared pixel for pixel.
fn retextured(id: u16, f: impl Fn(&Texture) -> Texture) -> MaterialTable {
    let mut out = MaterialTable::new();
    for (mid, m) in stock().iter() {
        let mut m: Material = m.clone();
        if mid.0 == id {
            m.texture = f(&m.texture);
        }
        out.add(m).expect("room");
    }
    out
}

fn same_size(t: &Texture, texel: impl Fn(&Texture, i32, i32) -> u32) -> Texture {
    let texels = (0..t.height() as i32)
        .flat_map(|y| (0..t.width() as i32).map(move |x| (x, y)))
        .map(|(x, y)| texel(t, x, y))
        .collect();
    Texture::new(t.width(), t.height(), texels).expect("same size")
}

/// Every texel white and solid, so the plate's square shows plain.
fn white(t: &Texture) -> Texture {
    same_size(t, |_, _, _| 0xffff_ffff)
}

/// The same colours with nothing covered: a plate that draws nothing.
fn clear(t: &Texture) -> Texture {
    let n = (t.width() * t.height()) as usize;
    same_size(t, |t, x, y| t.texel(x, y))
        .with_alpha(vec![0; n])
        .expect("same size")
}

/// The same colours, all solid: what a cutout would look like with no mask.
fn solid(t: &Texture) -> Texture {
    same_size(t, |t, x, y| t.texel(x, y) | 0xff00_0000)
}

#[test]
fn a_cutout_texel_shows_the_layer_behind_it_and_the_disc_is_the_planet() {
    let plate = level(Some((SKY_PLANET, 256)));
    let nothing = picture(&plate, retextured(SKY_PLANET, clear));
    let square = picture(&plate, retextured(SKY_PLANET, white));
    let planet = picture(&plate, stock());
    let (x0, y0, x1, y1) = changed(&nothing, &square).expect("the plate shows");
    assert!(
        x1 - x0 > 20 && y1 - y0 > 20 && x0 > 0 && y0 > 0 && x1 < W as i32 - 1 && y1 < H as i32 - 1
    );

    for (x, y) in [(x0, y0), (x1, y0), (x0, y1), (x1, y1)] {
        assert_eq!(px(&planet, x, y), px(&nothing, x, y), "corner {x},{y}");
    }
    let (cx, cy) = ((x0 + x1) / 2, (y0 + y1) / 2);
    assert_ne!(px(&planet, cx, cy), px(&nothing, cx, cy));

    // The texture's own middle, a few texels wide to ride over mip and filter.
    let t = load_texture(&dir(), "sky-planet").expect("loads");
    let mut want = [0i32; 3];
    for (x, y) in (124..132).flat_map(|y| (124..132).map(move |x| (x, y))) {
        for (w, c) in want.iter_mut().zip(channels(t.texel(x, y))) {
            *w += c / 64;
        }
    }
    let got = channels(px(&planet, cx, cy));
    for (g, w) in got.iter().zip(want) {
        assert!((g - w).abs() < 40, "{got:?} vs {want:?}");
    }
    // Where the plate is clear the picture is the backdrop's; a disc of it is not.
    let drawn = every_pixel().filter(|&(x, y)| px(&planet, x, y) != px(&nothing, x, y));
    let n = drawn.count() as f32;
    let area = ((x1 - x0) * (y1 - y0)) as f32;
    assert!(
        n > area * 0.5 && n < area * 0.9,
        "{n} of {area}: a disc, not a square"
    );
}

#[test]
fn soft_cloud_blends_with_the_layer_it_is_in_front_of() {
    // Clouds in a nearer layer (1), whose room is all viewport, over the stars.
    let nearer = |table: MaterialTable| {
        let mut l = level(None);
        l.brushes.push(room_at(2.0 * AWAY, 3, VIEW_FIRST));
        l.brushes.push(plate_at(2.0 * AWAY, 4, SKY_CLOUDS, 512));
        l.entities.push(layer((2.0 * AWAY) as f32, 1));
        picture(&l, table)
    };
    let behind = nearer(retextured(SKY_CLOUDS, clear));
    let soft = nearer(stock());
    let opaque = nearer(retextured(SKY_CLOUDS, solid));
    let between = every_pixel()
        .filter(|&(x, y)| {
            let c = px(&soft, x, y);
            c != px(&behind, x, y) && c != px(&opaque, x, y)
        })
        .count();
    assert!(between > 50, "{between} blended pixels");
    // A blend is a mix of the two: no channel strays outside them.
    for (x, y) in every_pixel() {
        let (a, b, s) = (px(&behind, x, y), px(&opaque, x, y), px(&soft, x, y));
        for ((a, b), s) in channels(a).into_iter().zip(channels(b)).zip(channels(s)) {
            assert!(
                s >= a.min(b) - 2 && s <= a.max(b) + 2,
                "{x},{y}: {a} {b} {s}"
            );
        }
    }
}

/// An RGBA PNG: one stored deflate block, so no compressor is needed.
fn rgba_png(w: u32, h: u32, rows: &[u8]) -> Vec<u8> {
    let mut z = vec![0x78, 0x01, 0x01];
    z.extend_from_slice(&(rows.len() as u16).to_le_bytes());
    z.extend_from_slice(&(!(rows.len() as u16)).to_le_bytes());
    z.extend_from_slice(rows);
    let (mut a, mut b) = (1u32, 0u32);
    for &x in rows {
        a = (a + u32::from(x)) % 65_521;
        b = (b + a) % 65_521;
    }
    z.extend_from_slice(&((b << 16) | a).to_be_bytes());
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&w.to_be_bytes());
    ihdr.extend_from_slice(&h.to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
    let mut f = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
    for (kind, body) in [(b"IHDR", ihdr), (b"IDAT", z), (b"IEND", Vec::new())] {
        f.extend_from_slice(&(body.len() as u32).to_be_bytes());
        f.extend_from_slice(kind);
        f.extend_from_slice(&body);
        f.extend_from_slice(&[0; 4]); // the decoder does not check CRCs
    }
    f
}

/// Raw scanlines of a `side` square RGBA image, alpha from `alpha(x, y)`.
fn rows(side: u32, alpha: impl Fn(u32, u32) -> u8) -> Vec<u8> {
    let mut out = Vec::new();
    for y in 0..side {
        out.push(0); // filter: none
        for x in 0..side {
            out.extend_from_slice(&[9, 9, 9, alpha(x, y)]);
        }
    }
    out
}

/// A directory that goes away with the test.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let d = std::env::temp_dir().join(format!("mscent_masked_{}_{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).expect("dir");
        Self(d)
    }

    fn load(&self, bytes: &[u8]) -> Result<Texture, TextureError> {
        std::fs::write(self.0.join("hostile.png"), bytes).expect("write");
        load_texture(&self.0, "hostile")
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn a_small_rgba_file_loads_masked_and_an_all_clear_one_still_loads() {
    let s = Scratch::new("small");
    let t = s
        .load(&rgba_png(
            4,
            4,
            &rows(4, |x, y| if x + y == 0 { 0 } else { 255 }),
        ))
        .expect("loads");
    assert!(t.is_masked());
    assert_eq!((t.alpha_at(0, 0), t.alpha_at(1, 0)), (0, 255));
    let clear = s.load(&rgba_png(4, 4, &rows(4, |_, _| 0))).expect("loads");
    assert!(clear.is_masked());
    let solid = s
        .load(&rgba_png(4, 4, &rows(4, |_, _| 255)))
        .expect("loads");
    assert!(
        !solid.is_masked(),
        "an RGBA file with no clear texel is opaque"
    );
    assert!(
        s.load(&rgba_png(3, 3, &rows(3, |_, _| 0))).is_err(),
        "3 is no power of 2"
    );
}

#[test]
fn a_truncated_or_forged_rgba_file_never_panics() {
    let s = Scratch::new("hostile");
    let good = std::fs::read(dir().join("sky-planet.png")).expect("stock file");
    let mut rng = Rng::new(7);
    for cut in [0, 8, 33, 40, 1000, good.len() / 2] {
        assert!(s.load(&good[..cut]).is_err(), "cut at {cut}");
    }
    for _ in 0..40 {
        let mut f = good.clone();
        for _ in 0..1 + rng.below(4) {
            let i = rng.below(f.len() as u32) as usize;
            f[i] = rng.below(256) as u8;
        }
        let _ = s.load(&f);
    }
    // A header that claims more pixels than the data holds.
    assert!(s.load(&rgba_png(4, 4, &[0; 10])).is_err());
}
