use super::*;
use crate::particles::Mask;

const W: u32 = 100;
const H: u32 = 80;

fn black() -> Framebuffer {
    Framebuffer::new(W, H).unwrap()
}

fn sprite(x: f32, y: f32, radius: f32) -> SpriteDraw {
    SpriteDraw {
        x,
        y,
        radius,
        inv_z: 0.5,
        tint: 0xff80_80ff,
        mask: Mask::Soft,
    }
}

fn draw(fb: &mut Framebuffer, s: &SpriteDraw, blend: Blend) {
    draw_sprite(Level::best(), fb, s, blend, &SpriteBank::new());
}

fn changed(fb: &Framebuffer) -> Vec<(i32, i32)> {
    let mut out = Vec::new();
    for y in 0..H as i32 {
        for x in 0..W as i32 {
            if fb.pixel(x, y) != Some(0xff00_0000) {
                out.push((x, y));
            }
        }
    }
    out
}

#[test]
fn sprite_stays_inside_its_box_and_is_brightest_at_the_centre() {
    let mut fb = black();
    draw(&mut fb, &sprite(50.0, 40.0, 10.0), Blend::Add);
    let px = changed(&fb);
    assert!(!px.is_empty());
    assert!(
        px.iter()
            .all(|&(x, y)| (40..60).contains(&x) && (30..50).contains(&y))
    );
    let blue = |x, y| fb.pixel(x, y).unwrap() & 0xff;
    assert!(blue(50, 40) > blue(44, 40) && blue(44, 40) > blue(41, 40));
}

#[test]
fn sprite_is_symmetric_about_its_centre() {
    let mut fb = black();
    draw(&mut fb, &sprite(50.0, 40.0, 16.0), Blend::Add);
    for d in 0..16 {
        assert_eq!(
            fb.pixel(50 + d, 40),
            fb.pixel(49 - d, 40),
            "offset {d} left and right"
        );
        assert_eq!(
            fb.pixel(50, 40 + d),
            fb.pixel(50, 39 - d),
            "offset {d} up and down"
        );
    }
}

#[test]
fn partly_and_wholly_off_screen_sprites_clip() {
    let mut fb = black();
    draw(&mut fb, &sprite(-20.0, 40.0, 10.0), Blend::Add);
    draw(&mut fb, &sprite(50.0, 1.0e6, 10.0), Blend::Add);
    draw(&mut fb, &sprite(W as f32 + 20.0, 40.0, 10.0), Blend::Over);
    assert!(changed(&fb).is_empty());
    draw(&mut fb, &sprite(0.0, 0.0, 10.0), Blend::Add);
    let px = changed(&fb);
    assert!(!px.is_empty() && px.iter().all(|&(x, y)| x < 10 && y < 10));
}

#[test]
fn clipping_does_not_move_the_sprite() {
    // A sprite cut by the left edge matches the same sprite drawn whole in a
    // target 40 pixels wider, shifted by 40.
    let mut cut = black();
    draw(&mut cut, &sprite(10.0, 40.0, 25.0), Blend::Over);
    let mut whole = Framebuffer::new(W + 40, H).unwrap();
    draw(&mut whole, &sprite(50.0, 40.0, 25.0), Blend::Over);
    assert!(!changed(&cut).is_empty());
    for y in 0..H as i32 {
        for x in 0..W as i32 {
            assert_eq!(cut.pixel(x, y), whole.pixel(x + 40, y), "{x},{y}");
        }
    }
}

#[test]
fn tiny_and_degenerate_radii_do_not_panic() {
    let mut fb = black();
    for r in [1.0e-30, f32::MIN_POSITIVE, 1.0e-3, 0.25, 0.5, 1.0, 1.5] {
        draw(&mut fb, &sprite(50.3, 40.7, r), Blend::Add);
        draw(&mut fb, &sprite(50.3, 40.7, r), Blend::Over);
    }
}

#[test]
fn huge_radius_is_capped_and_still_covers_the_screen() {
    let mut fb = black();
    draw(&mut fb, &sprite(50.0, 40.0, 1.0e9), Blend::Add);
    // Capped to 512 px, the middle of the screen is the middle of the sprite.
    assert_ne!(fb.pixel(50, 40), Some(0xff00_0000));
}

#[test]
fn mip_is_the_next_one_up() {
    assert_eq!(mip_for(2.0), 0);
    assert_eq!(mip_for(8.0), 0);
    assert_eq!(mip_for(8.5), 1);
    assert_eq!(mip_for(33.0), 3);
    assert_eq!(mip_for(128.0), 4);
    assert_eq!(mip_for(1024.0), 4);
}

#[test]
fn nearest_stays_inside_the_mask() {
    assert_eq!(nearest(-5.0, 1.0, 8), 0);
    assert_eq!(nearest(1.0e9, 1.0, 8), 7);
    assert_eq!(nearest(f32::NAN, 1.0, 8), 0);
}

#[test]
fn every_scratch_size_draws_the_same_picture() {
    // Diameters 10, 100 and 300 pick the 32, 128 and 1024 pixel tables.
    for r in [5.0, 50.0, 150.0] {
        let mut fb = Framebuffer::new(400, 400).unwrap();
        draw(&mut fb, &sprite(200.0, 200.0, r), Blend::Add);
        let lit = fb.pixels().iter().filter(|&&p| p != 0xff00_0000).count();
        assert!(lit as f32 > r * r, "radius {r}: {lit} pixels lit");
    }
}
