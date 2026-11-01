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
