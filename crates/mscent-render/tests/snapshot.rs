//! Offscreen snapshot tests. Each scene is rendered with the real rasterizer
//! and compared pixel for pixel with a reference under `tests/ref/`, plus
//! direct pixel assertions that do not depend on the reference.
//!
//! To (re)generate references after an intended rendering change, run with
//! `MSCENT_BLESS=1` and review the diff of the `.ppm` files.

use std::path::PathBuf;

use mscent_math::Vec3;
use mscent_render::{
    Camera, Fog, Framebuffer, MaterialTable, Texture, Vertex, draw_polygon_flat, draw_text,
    fill_rect, stroke_rect,
};

fn ref_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/ref")
        .join(name)
}

fn to_ppm(fb: &Framebuffer) -> Vec<u8> {
    let mut out = format!("P6\n{} {}\n255\n", fb.width(), fb.height()).into_bytes();
    for &p in fb.pixels() {
        out.extend_from_slice(&[(p >> 16) as u8, (p >> 8) as u8, p as u8]);
    }
    out
}

fn assert_matches_reference(fb: &Framebuffer, name: &str) {
    let actual = to_ppm(fb);
    let path = ref_path(name);
    if std::env::var_os("MSCENT_BLESS").is_some() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &actual).unwrap();
        return;
    }
    let expected = std::fs::read(&path).unwrap_or_else(|e| {
        panic!(
            "missing reference {} ({e}); run with MSCENT_BLESS=1",
            path.display()
        )
    });
    assert_eq!(expected.len(), actual.len(), "{name}: size changed");
    let differing = expected.iter().zip(&actual).filter(|(a, b)| a != b).count();
    assert_eq!(
        differing, 0,
        "{name}: {differing} bytes differ from the reference"
    );
}

fn brick() -> Texture {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets/textures");
    let table = MaterialTable::load_dir(&dir).unwrap();
    table
        .get(table.find("brick").unwrap())
        .unwrap()
        .texture
        .clone()
}

fn camera(eye: Vec3, target: Vec3, w: u32, h: u32) -> Camera {
    Camera::look_at(eye, target, Vec3::new(0.0, 1.0, 0.0), 1.2, 0.1, 100.0, w, h).unwrap()
}

fn wall(x0: f32, x1: f32, z0: f32, z1: f32, repeats: f32) -> [Vertex; 4] {
    [
        Vertex::new(Vec3::new(x0, 1.0, z0), 0.0, 0.0),
        Vertex::new(Vec3::new(x1, 1.0, z1), repeats, 0.0),
        Vertex::new(Vec3::new(x1, -1.0, z1), repeats, repeats * 0.5),
        Vertex::new(Vec3::new(x0, -1.0, z0), 0.0, repeats * 0.5),
    ]
}

#[test]
fn brick_quad_facing_the_camera() {
    let tex = brick();
    let cam = camera(Vec3::new(0.0, 0.0, 0.0), Vec3::new(0.0, 0.0, -1.0), 96, 72);
    let mut fb = Framebuffer::new(96, 72).unwrap();
    fb.clear(0xff20_2040);
    // 1 repeat across 2 world units at depth 2 covers the middle of the view.
    let q = [
        Vertex::new(Vec3::new(-1.0, 1.0, -2.0), 0.0, 0.0),
        Vertex::new(Vec3::new(1.0, 1.0, -2.0), 1.0, 0.0),
        Vertex::new(Vec3::new(1.0, -1.0, -2.0), 1.0, 1.0),
        Vertex::new(Vec3::new(-1.0, -1.0, -2.0), 0.0, 1.0),
    ];
    let stats = draw_polygon_flat(&mut fb, &cam, &q, &tex, 256, None);

    // Direct readback: the quad spans screen x in (48 +- f/2) with
    // f = 36 / tan(0.6) ~ 52.6; pixel (48, 36) is centred half a pixel past
    // the quad centre, i.e. u = 0.5 + 0.5 / 105.2, texel 65.2.
    assert!(
        stats.pixels > 2000,
        "quad covers a large area, got {}",
        stats.pixels
    );
    assert_eq!(
        fb.pixel(2, 2),
        Some(0xff20_2040),
        "background outside the quad"
    );
    let centre = fb.pixel(48, 36).unwrap();
    let texel = tex.texel(65, 65);
    assert_eq!(
        centre, texel,
        "fronto-parallel centre pixel is the centre texel"
    );
    assert_matches_reference(&fb, "brick_quad.ppm");
}

#[test]
fn brick_wall_in_perspective_with_fog() {
    let tex = brick();
    let cam = camera(Vec3::new(0.0, 0.0, 0.0), Vec3::new(1.0, 0.0, -3.0), 96, 72);
    let mut fb = Framebuffer::new(96, 72).unwrap();
    fb.clear_sky(0xff30_5080, 0xff90_b0d0);
    let fog = Fog::new(0xff90_b0d0, 2.0, 14.0);
    let wall = wall(-1.0, 6.0, -4.0, -12.0, 3.0);
    let stats = draw_polygon_flat(&mut fb, &cam, &wall, &tex, 256, fog);
    assert!(stats.pixels > 500);
    // The far end of the wall is fogged toward the sky colour: it is closer
    // to the fog colour than the near end is.
    let dist =
        |p: u32| ((p >> 16 & 0xff) as i32 - 0x90).abs() + ((p >> 8 & 0xff) as i32 - 0xb0).abs();
    let (mut near_d, mut far_d) = (i32::MAX, i32::MAX);
    for x in 0..96 {
        if fb.depth_at(x, 36).is_some_and(|d| d > 0.0) {
            let d = 1.0 / fb.depth_at(x, 36).unwrap();
            let c = dist(fb.pixel(x, 36).unwrap());
            if d < 6.0 {
                near_d = near_d.min(c)
            } else if d > 10.0 {
                far_d = far_d.min(c)
            }
        }
    }
    assert!(far_d < near_d + 1, "far {far_d} vs near {near_d}");
    assert_eq!(
        fb.pixel(0, 0),
        Some(0xff30_5080),
        "sky gradient starts at its top colour"
    );
    assert_matches_reference(&fb, "brick_wall_fog.ppm");
}

#[test]
fn flat_lit_fallback_scales_brightness() {
    let cam = camera(Vec3::new(0.0, 0.0, 0.0), Vec3::new(0.0, 0.0, -1.0), 32, 32);
    let q = [
        Vertex::new(Vec3::new(-3.0, 3.0, -2.0), 0.0, 0.0),
        Vertex::new(Vec3::new(3.0, 3.0, -2.0), 1.0, 0.0),
        Vertex::new(Vec3::new(3.0, -3.0, -2.0), 1.0, 1.0),
        Vertex::new(Vec3::new(-3.0, -3.0, -2.0), 0.0, 1.0),
    ];
    let solid = Texture::solid(0xff80_6040);
    let mut fb = Framebuffer::new(32, 32).unwrap();
    draw_polygon_flat(&mut fb, &cam, &q, &solid, 128, None);
    assert!(
        fb.pixels().iter().all(|&p| p == 0xff40_3020),
        "half light halves each channel"
    );
    fb.clear(0);
    draw_polygon_flat(&mut fb, &cam, &q, &solid, 512, None);
    assert!(
        fb.pixels().iter().all(|&p| p == 0xffff_c080),
        "double light saturates at 255"
    );
    assert_matches_reference(&fb, "flat_overbright.ppm");
}

#[test]
fn text_rects_and_clipping_at_the_screen_edge() {
    let mut fb = Framebuffer::new(80, 48).unwrap();
    fb.clear(0xff10_1018);
    draw_text(&mut fb, 2, 2, "MINISCENT", 0xffff_ffff, 1);
    draw_text(&mut fb, 2, 12, "Score 42", 0xff40_ff40, 2);
    draw_text(&mut fb, 2, 30, "x\ny", 0xffff_c040, 3);
    stroke_rect(&mut fb, 0, 0, 80, 48, 1, 0xffff_0000);
    fill_rect(&mut fb, 60, 30, 40, 40, 0x80ff_ffff);
    draw_text(&mut fb, 70, 40, "EDGE", 0xff00_0000, 2);
    // Border pixels, a text pixel and the clipped translucent box.
    assert_eq!(fb.pixel(0, 0), Some(0xffff_0000));
    assert_eq!(fb.pixel(79, 0), Some(0xffff_0000));
    assert_eq!(fb.pixel(0, 47), Some(0xffff_0000));
    assert_eq!(
        fb.pixel(2, 2),
        Some(0xffff_ffff),
        "top-left pixel of the 'M' glyph (0x63 sets bit 0)"
    );
    assert_eq!(fb.pixel(4, 2), Some(0xff10_1018), "bit 2 of 0x63 is clear");
    let tint = fb.pixel(65, 33).unwrap();
    assert!(
        tint & 0xff > 0x70,
        "translucent white lifted the background: {tint:#x}"
    );
    assert_matches_reference(&fb, "text_and_clip.ppm");
}
