//! End-to-end checks of the level pipeline on the showcase level, reading
//! real pixels.

use std::path::Path;

use mscent_math::Vec3;
use mscent_render::{BuildConfig, Framebuffer, LevelRenderer, MaterialTable, Quality, View};
use mscent_world::showcase_level;

fn renderer() -> LevelRenderer {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/textures");
    let materials = MaterialTable::load_dir(&dir).expect("textures");
    LevelRenderer::from_level(&showcase_level(), materials, &BuildConfig::default())
        .expect("showcase builds")
}

fn frame(r: &LevelRenderer, view: &View, q: Quality) -> Vec<u32> {
    let mut fb = Framebuffer::new(160, 90).expect("fb");
    r.render(&mut fb, view, q, r.actors(), None)
        .expect("render");
    fb.pixels().to_vec()
}

fn mean_rgb(px: &[u32]) -> [f32; 3] {
    let n = px.len() as f32;
    let sum = |s: u32| px.iter().map(|p| ((p >> s) & 0xff) as f32).sum::<f32>() / n;
    [sum(16), sum(8), sum(0)]
}

#[test]
fn rooms_take_the_colour_of_their_lights() {
    let r = renderer();
    // Looking at the floor of the blue hall and of the red room.
    let blue = frame(
        &r,
        &View::from_angles(Vec3::new(36.0, 4.0, 3.0), 0.5, -0.8, 0.0),
        Quality::Medium,
    );
    let red = frame(
        &r,
        &View::from_angles(Vec3::new(18.0, 40.0, 3.0), 0.5, -0.8, 0.0),
        Quality::Medium,
    );
    let (b, rd) = (mean_rgb(&blue), mean_rgb(&red));
    assert!(b[2] > b[0] * 1.2, "blue hall should be bluish: {b:?}");
    assert!(rd[0] > rd[2] * 1.5, "red room should be reddish: {rd:?}");
}

#[test]
fn no_frame_is_mostly_black_or_flat() {
    let r = renderer();
    let view = View::from_angles(Vec3::new(3.0, 3.0, 2.5), -0.78, 0.0, 0.0);
    let px = frame(&r, &view, Quality::High);
    let black = px.iter().filter(|&&p| p & 0xff_ffff == 0).count();
    assert!(
        black * 50 < px.len(),
        "{black} black pixels of {}",
        px.len()
    );
    let distinct: std::collections::HashSet<u32> = px.iter().copied().collect();
    assert!(distinct.len() > 500, "only {} colours", distinct.len());
}

#[test]
fn quality_levels_and_roll_change_the_picture() {
    let r = renderer();
    let view = View::from_angles(Vec3::new(8.0, 17.0, 3.0), 0.4, 0.05, 0.0);
    let low = frame(&r, &view, Quality::Low);
    let med = frame(&r, &view, Quality::Medium);
    let high = frame(&r, &view, Quality::High);
    assert_ne!(low, med, "bump mapping must change the picture");
    assert_ne!(med, high, "specular must change the picture");
    let rolled = View::from_angles(Vec3::new(8.0, 17.0, 3.0), 0.4, 0.05, 0.5);
    assert_ne!(frame(&r, &rolled, Quality::Low), low);
}

#[test]
fn pillar_casts_a_shadow_on_the_floor() {
    let r = renderer();
    // Straight down at the floor behind the pillar (north east of it, as
    // seen from the warm light) and at a spot the light reaches freely.
    let down = |x: f32, y: f32| {
        let v = View::from_angles(
            Vec3::new(x, y, 2.0),
            0.0,
            -core::f32::consts::FRAC_PI_2 + 0.01,
            0.0,
        );
        let mut fb = Framebuffer::new(32, 32).expect("fb");
        r.render(&mut fb, &v, Quality::Low, &[], None)
            .expect("render");
        let px = fb.pixels();
        mean_rgb(&px[32 * 12..32 * 20])
    };
    let shadowed = down(16.0, 16.0);
    let lit = down(9.0, 4.0);
    let lum = |c: [f32; 3]| c[0] + c[1] + c[2];
    assert!(
        lum(shadowed) * 1.4 < lum(lit),
        "shadow {shadowed:?} vs lit {lit:?}"
    );
}

fn mean_abs_diff(a: &[u32], b: &[u32]) -> f32 {
    let sum: u32 = a
        .iter()
        .zip(b)
        .map(|(x, y)| {
            (0..3)
                .map(|c| ((x >> (8 * c)) & 0xff).abs_diff((y >> (8 * c)) & 0xff))
                .sum::<u32>()
        })
        .sum();
    sum as f32 / (a.len() * 3) as f32
}

#[test]
fn bump_and_specular_are_clearly_visible_not_just_nonzero() {
    let r = renderer();
    // The steel hall: glossy metal walls lit by the blue lamp.
    let view = View::from_angles(Vec3::new(34.0, 2.0, 3.5), -0.7, -0.15, 0.0);
    let (low, med, high) = (
        frame(&r, &view, Quality::Low),
        frame(&r, &view, Quality::Medium),
        frame(&r, &view, Quality::High),
    );
    let (bump, spec) = (mean_abs_diff(&low, &med), mean_abs_diff(&med, &high));
    assert!(
        bump > 2.0,
        "bump mapping changes the picture by only {bump}"
    );
    assert!(spec > 0.8, "specular changes the picture by only {spec}");
    let bright = |p: &[u32]| mean_rgb(p).iter().sum::<f32>();
    assert!(bright(&high) > bright(&med), "specular only adds light");
}

#[test]
fn distance_fog_pulls_far_walls_toward_the_fog_colour() {
    let r = renderer();
    let cfg = r.config();
    // Looking down the long A-B corridor: the far wall is 45 units away.
    let view = View::from_angles(
        Vec3::new(3.0, 10.0, 2.0),
        -core::f32::consts::FRAC_PI_2,
        0.0,
        0.0,
    );
    let px = frame(&r, &view, Quality::High);
    let centre: Vec<u32> = (70..90).map(|x| px[45 * 160 + x]).collect();
    let fog = cfg.sky_bottom;
    let dist = |c: &[u32]| -> f32 {
        let m = mean_rgb(c);
        (0..3)
            .map(|k| (m[k] - ((fog >> (16 - 8 * k)) & 0xff) as f32).abs())
            .sum()
    };
    let near_view = View::from_angles(
        Vec3::new(18.0, 10.0, 2.0),
        -core::f32::consts::FRAC_PI_2,
        0.0,
        0.0,
    );
    let near = frame(&r, &near_view, Quality::High);
    let near_row: Vec<u32> = (70..90).map(|x| near[45 * 160 + x]).collect();
    assert!(
        dist(&centre) < dist(&near_row),
        "far row is not closer to the fog colour"
    );
}
