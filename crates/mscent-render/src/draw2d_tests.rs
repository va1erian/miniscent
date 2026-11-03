use super::*;

const RED: u32 = 0xffff_0000;
const BLUE: u32 = 0xff00_00ff;

fn fb(w: u32, h: u32) -> Framebuffer {
    let mut f = Framebuffer::new(w, h).unwrap();
    f.clear(0xff00_0000);
    f
}

fn count(f: &Framebuffer, color: u32) -> usize {
    f.pixels().iter().filter(|&&p| p == color).count()
}

#[test]
fn rect_inside_covers_exactly_its_area() {
    let mut f = fb(10, 10);
    fill_rect(&mut f, 2, 3, 4, 5, RED);
    assert_eq!(count(&f, RED), 20);
    assert_eq!(f.pixel(2, 3), Some(RED));
    assert_eq!(f.pixel(5, 7), Some(RED));
    assert_eq!(f.pixel(6, 7), Some(0xff00_0000));
    assert_eq!(f.pixel(5, 8), Some(0xff00_0000));
}

#[test]
fn rect_clips_at_every_edge() {
    let mut f = fb(10, 10);
    fill_rect(&mut f, -3, -3, 6, 6, RED);
    assert_eq!(count(&f, RED), 9, "top-left corner keeps a 3x3 patch");
    let mut f = fb(10, 10);
    fill_rect(&mut f, 8, 8, 6, 6, RED);
    assert_eq!(count(&f, RED), 4, "bottom-right corner keeps 2x2");
    assert_eq!(f.pixel(9, 9), Some(RED));
}

#[test]
fn rect_far_off_screen_or_huge_is_safe() {
    let mut f = fb(10, 10);
    fill_rect(&mut f, i32::MAX, i32::MAX, u32::MAX, u32::MAX, RED);
    fill_rect(&mut f, i32::MIN, i32::MIN, 5, 5, RED);
    fill_rect(&mut f, -5, 0, 0, 4, RED);
    assert_eq!(count(&f, RED), 0);
    fill_rect(&mut f, i32::MIN, i32::MIN, u32::MAX, u32::MAX, RED);
    assert_eq!(
        count(&f, RED),
        100,
        "a rect spanning the whole screen fills it"
    );
}

#[test]
fn translucent_fill_blends() {
    let mut f = fb(2, 2);
    fill_rect(&mut f, 0, 0, 1, 1, 0x80ff_ffff);
    let p = f.pixel(0, 0).unwrap();
    assert_eq!(p >> 24, 255);
    assert!(((p & 0xff) as i32 - 128).abs() <= 1);
    fill_rect(&mut f, 1, 1, 1, 1, 0x00ff_ffff);
    assert_eq!(f.pixel(1, 1), Some(0xff00_0000), "alpha 0 is invisible");
}

#[test]
fn stroke_draws_a_frame_without_overlap() {
    let mut f = fb(10, 10);
    stroke_rect(&mut f, 1, 1, 6, 5, 1, RED);
    assert_eq!(count(&f, RED), 6 * 5 - 4 * 3);
    assert_eq!(f.pixel(3, 3), Some(0xff00_0000), "interior untouched");
    let mut f = fb(10, 10);
    stroke_rect(&mut f, 0, 0, 3, 3, 2, RED);
    assert_eq!(count(&f, RED), 9, "no interior left, so it is filled");
}

#[test]
fn sprite_blit_honours_alpha_and_clipping() {
    let s = Sprite::new(2, 2, vec![RED, 0x0000_ff00, BLUE, RED]).unwrap();
    let mut f = fb(4, 4);
    blit(&mut f, &s, 1, 1);
    assert_eq!(f.pixel(1, 1), Some(RED));
    assert_eq!(
        f.pixel(2, 1),
        Some(0xff00_0000),
        "transparent texel skipped"
    );
    assert_eq!(f.pixel(1, 2), Some(BLUE));
    assert_eq!(f.pixel(2, 2), Some(RED));
    // Hanging off the top-left: only the bottom-right texel is visible.
    let mut f = fb(4, 4);
    blit(&mut f, &s, -1, -1);
    assert_eq!(count(&f, RED), 1);
    assert_eq!(f.pixel(0, 0), Some(RED));
    // Off the bottom-right.
    let mut f = fb(4, 4);
    blit(&mut f, &s, 3, 3);
    assert_eq!(f.pixel(3, 3), Some(RED));
    assert_eq!(count(&f, RED), 1);
    blit(&mut f, &s, i32::MIN, i32::MAX);
}

#[test]
fn sprite_rejects_wrong_sizes() {
    assert!(Sprite::new(2, 2, vec![0; 3]).is_none());
    assert!(Sprite::new(0, 2, vec![]).is_none());
}

#[test]
fn text_matches_the_font_bitmap_at_two_scales() {
    // 'A' row 0 is 0x0C: pixels 2 and 3 set.
    let mut f = fb(16, 16);
    draw_text(&mut f, 0, 0, "A", RED, 1);
    assert_eq!(f.pixel(2, 0), Some(RED));
    assert_eq!(f.pixel(3, 0), Some(RED));
    assert_eq!(f.pixel(1, 0), Some(0xff00_0000));
    assert_eq!(f.pixel(4, 0), Some(0xff00_0000));
    let lit_1x = count(&f, RED);
    assert_eq!(
        lit_1x,
        glyph('A')
            .iter()
            .map(|r| r.count_ones() as usize)
            .sum::<usize>()
    );

    let mut f = fb(16, 16);
    draw_text(&mut f, 0, 0, "A", RED, 2);
    assert_eq!(count(&f, RED), lit_1x * 4);
    assert_eq!(f.pixel(4, 0), Some(RED));
    assert_eq!(f.pixel(7, 1), Some(RED));
    assert_eq!(f.pixel(8, 0), Some(0xff00_0000));
}

#[test]
fn text_advances_and_wraps_lines() {
    assert_eq!(text_size("ab\nabc", 1), (24, 16));
    assert_eq!(text_size("", 3), (0, 24));
    let mut f = fb(32, 32);
    draw_text(&mut f, 0, 0, "I\nI", RED, 1);
    let top = (0..8)
        .filter(|&y| (0..8).any(|x| f.pixel(x, y) == Some(RED)))
        .count();
    let bottom = (8..16)
        .filter(|&y| (0..8).any(|x| f.pixel(x, y) == Some(RED)))
        .count();
    assert_eq!((top, bottom), (7, 7));
}

#[test]
fn text_clips_at_the_screen_edge() {
    let mut f = fb(8, 8);
    draw_text(&mut f, 4, 4, "WW", RED, 1);
    assert!(count(&f, RED) > 0);
    draw_text(&mut f, -100, -100, "hello", RED, 3);
    draw_text(&mut f, i32::MAX - 3, i32::MAX - 3, "x", RED, u32::MAX);
    draw_text(&mut f, 0, 0, "zero scale", RED, 0);
}

#[test]
fn text_size_with_huge_scale_saturates() {
    assert_eq!(text_size("ab\ncd", u32::MAX), (u32::MAX, u32::MAX));
}

#[test]
fn drawing_into_a_band_uses_screen_rows() {
    let mut full = fb(8, 8);
    fill_rect(&mut full, 1, 5, 3, 2, RED);
    let mut band = Framebuffer::new_band(8, 4, 4, 8).unwrap();
    band.clear(0xff00_0000);
    fill_rect(&mut band, 1, 5, 3, 2, RED);
    for y in 4..8 {
        for x in 0..8 {
            assert_eq!(band.pixel(x, y), full.pixel(x, y), "({x},{y})");
        }
    }
    // Nothing above the band's first row may leak into it.
    let mut band = Framebuffer::new_band(8, 4, 4, 8).unwrap();
    band.clear(0xff00_0000);
    fill_rect(&mut band, 0, 0, 8, 4, RED);
    assert_eq!(band.pixels().iter().filter(|&&p| p == RED).count(), 0);
}

#[test]
fn a_layer_composited_over_a_frame_is_drawing_on_the_frame() {
    let scene = |f: &mut Framebuffer| {
        for (i, p) in f.pixels_mut().iter_mut().enumerate() {
            *p = 0xff00_0000 | (i as u32 * 37 % 256) << 16 | (i as u32 * 91 % 256) << 8 | 200;
        }
    };
    let draw = |f: &mut Framebuffer| {
        fill_rect(f, 1, 1, 6, 6, 0x80ff_2010);
        fill_rect(f, 3, 3, 6, 6, 0x4010_ff40);
        fill_rect(f, 5, 0, 2, 9, RED);
        draw_text(f, 0, 2, "A", 0xc0ff_ffff, 1);
    };
    let mut direct = fb(10, 10);
    scene(&mut direct);
    draw(&mut direct);
    let mut layer = Framebuffer::new(10, 10).unwrap();
    layer.clear(0);
    draw(&mut layer);
    let mut frame = fb(10, 10);
    scene(&mut frame);
    layer_over(&mut frame, &layer);
    for (&d, &c) in direct.pixels().iter().zip(frame.pixels()) {
        for shift in [16, 8, 0] {
            let apart = ((d >> shift) & 255).abs_diff((c >> shift) & 255);
            assert!(apart <= 2, "{d:08x} drawn, {c:08x} composited");
        }
    }
    assert!(
        direct.pixels().iter().all(|p| p >> 24 == 255),
        "a frame stays opaque"
    );
}
