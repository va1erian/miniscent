use crate::camera::Camera;
use crate::framebuffer::Framebuffer;
use crate::raster::{Vertex, draw_polygon_flat};
use crate::texture::Texture;
use mscent_math::Vec3;

const BACK: u32 = 0xff20_4060;
const RED: u32 = 0xffff_0000;
const GREEN: u32 = 0xff00_ff00;

/// 64x64 target, 90 degree fov (32 px focal length), looking down -Z.
fn setup() -> (Framebuffer, Camera) {
    let cam = Camera::look_at(
        Vec3::ZERO,
        Vec3::new(0.0, 0.0, -1.0),
        Vec3::new(0.0, 1.0, 0.0),
        core::f32::consts::FRAC_PI_2,
        0.1,
        200.0,
        64,
        64,
    )
    .unwrap();
    (Framebuffer::new(64, 64).unwrap(), cam)
}

/// A square of half-size `half` at depth `-z`, all of the texture across it.
fn quad(half: f32, z: f32) -> [Vertex; 4] {
    [
        Vertex::new(Vec3::new(-half, half, z), 0.0, 0.0),
        Vertex::new(Vec3::new(half, half, z), 1.0, 0.0),
        Vertex::new(Vec3::new(half, -half, z), 1.0, 1.0),
        Vertex::new(Vec3::new(-half, -half, z), 0.0, 1.0),
    ]
}

/// Texels: clear red, solid green / half red, quarter green.
fn cutout() -> Texture {
    Texture::new(2, 2, vec![RED, GREEN, RED, GREEN])
        .unwrap()
        .with_alpha(vec![0, 255, 128, 64])
        .unwrap()
}

/// A backdrop at 1/z = 0.25 with the cutout in front of it at 0.5.
fn drawn(tex: &Texture) -> (Framebuffer, u32) {
    let (mut fb, cam) = setup();
    let back = Texture::new(1, 1, vec![BACK]).unwrap();
    draw_polygon_flat(&mut fb, &cam, &quad(8.0, -4.0), &back, 256, None);
    let st = draw_polygon_flat(&mut fb, &cam, &quad(2.0, -2.0), tex, 256, None);
    (fb, st.pixels)
}

#[test]
fn a_clear_texel_shows_what_is_behind_and_leaves_depth_alone() {
    let (fb, _) = drawn(&cutout());
    assert_eq!(fb.pixel(16, 16), Some(BACK));
    assert_eq!(fb.depth_at(16, 16), Some(0.25));
}

#[test]
fn a_solid_texel_writes_colour_and_depth() {
    let (fb, _) = drawn(&cutout());
    assert_eq!(fb.pixel(48, 16), Some(GREEN));
    assert_eq!(fb.depth_at(48, 16), Some(0.5));
}

#[test]
fn a_partial_texel_blends_over_what_is_behind_and_leaves_depth_alone() {
    let (fb, _) = drawn(&cutout());
    // Alpha 128 of red over the backdrop: halfway, per channel.
    let half = fb.pixel(16, 48).unwrap();
    for (shift, back, front) in [(16, 0x20, 0xff), (8, 0x40, 0), (0, 0x60, 0)] {
        let c = (half >> shift & 0xff) as i32;
        assert!((c - (back + front) / 2).abs() <= 2, "{shift}: {c}");
    }
    assert_eq!(fb.depth_at(16, 48), Some(0.25));
    // Alpha 64 is a quarter of the way to green.
    let quarter = fb.pixel(48, 48).unwrap();
    assert!(((quarter >> 8 & 0xff) as i32 - (0x40 + 0xbf / 4)).abs() <= 2);
}

#[test]
fn clear_texels_are_not_counted_as_written() {
    let (_, written) = drawn(&cutout());
    assert_eq!(written, 64 * 64 * 3 / 4, "three of the four texels");
}

#[test]
fn what_is_drawn_later_still_shows_through_a_soft_edge() {
    let (mut fb, cam) = setup();
    let st = draw_polygon_flat(&mut fb, &cam, &quad(2.0, -2.0), &cutout(), 256, None);
    assert_eq!(st.pixels, 64 * 64 * 3 / 4);
    let back = Texture::new(1, 1, vec![BACK]).unwrap();
    draw_polygon_flat(&mut fb, &cam, &quad(8.0, -4.0), &back, 256, None);
    assert_eq!(fb.pixel(16, 16), Some(BACK), "clear: behind is drawn");
    assert_eq!(fb.pixel(16, 48), Some(BACK), "soft: depth was not written");
    assert_eq!(fb.pixel(48, 16), Some(GREEN), "solid: it won");
}

#[test]
fn a_filtered_cutout_samples_alpha_bilinearly() {
    let tex = cutout().filtered();
    let (fb, _) = drawn(&tex);
    // Across the solid/clear seam the alpha ramps, so some pixel is neither.
    let seam = (24..40).map(|x| fb.pixel(x, 16).unwrap());
    assert!(seam.clone().any(|c| c != BACK && c != GREEN), "{seam:?}");
    assert!(seam.clone().all(|c| c != RED));
}

#[test]
fn an_opaque_texture_takes_the_plain_loop() {
    let tex = Texture::new(2, 2, vec![RED, GREEN, RED, GREEN])
        .unwrap()
        .with_alpha(vec![255; 4])
        .unwrap();
    assert!(!tex.is_masked());
    let (fb, written) = drawn(&tex);
    assert_eq!(written, 64 * 64);
    assert_eq!(fb.pixel(16, 16), Some(RED));
}
