#![forbid(unsafe_code)]

//! Alignment and clip regressions for the software canvas, kept out of the
//! main `tests` module so neither file grows past the repo's file-size limit.

use xui_core::backend::TextAlign;
use xui_core::geometry::Rect;
use xui_core::image::Image;
use xui_core::{Canvas, Color, Corner, Dip, TextStyle};

use crate::{RgbaImage, Surface};

/// The x-range of the pixels in `region` that differ from `background`, if any.
fn ink_bounds(image: &RgbaImage, region: Rect, background: [u8; 3]) -> Option<(i32, i32)> {
    let mut min_x = i32::MAX;
    let mut max_x = i32::MIN;
    for y in region.top..region.bottom {
        for x in region.left..region.right {
            let Some([r, g, b, _]) = image.pixel(x as u32, y as u32) else {
                continue;
            };
            if [r, g, b] != background {
                min_x = min_x.min(x);
                max_x = max_x.max(x);
            }
        }
    }
    (min_x <= max_x).then_some((min_x, max_x))
}

/// Whether the centre of pixel `(x, y)` lies inside the rounded rectangle
/// `rect`/`radius`, grown by `slack` pixels.
fn inside_rounded(rect: Rect, radius: f32, x: i32, y: i32, slack: f32) -> bool {
    let (cx, cy) = (x as f32 + 0.5, y as f32 + 0.5);
    if cx < rect.left as f32 - slack
        || cy < rect.top as f32 - slack
        || cx > rect.right as f32 + slack
        || cy > rect.bottom as f32 + slack
    {
        return false;
    }
    for (corner_x, corner_y, out_x, out_y) in [
        (
            rect.left as f32 + radius,
            rect.top as f32 + radius,
            -1.0,
            -1.0,
        ),
        (
            rect.right as f32 - radius,
            rect.top as f32 + radius,
            1.0,
            -1.0,
        ),
        (
            rect.right as f32 - radius,
            rect.bottom as f32 - radius,
            1.0,
            1.0,
        ),
        (
            rect.left as f32 + radius,
            rect.bottom as f32 - radius,
            -1.0,
            1.0,
        ),
    ] {
        if (cx - corner_x) * out_x > 0.0 && (cy - corner_y) * out_y > 0.0 {
            let (dx, dy) = (cx - corner_x, cy - corner_y);
            if dx * dx + dy * dy > (radius + slack) * (radius + slack) {
                return false;
            }
        }
    }
    true
}

#[test]
fn centred_and_end_aligned_text_are_not_left_aligned() {
    const W: i32 = 240;
    let background = Color::rgb(255, 255, 255);
    let mut surface = Surface::new(W as u32, 96);
    surface.fill(background);

    let mut end = TextStyle::new(Color::rgb(0, 0, 0), Dip(18.0));
    end.align = TextAlign::End;
    let styles = [
        TextStyle::new(Color::rgb(0, 0, 0), Dip(18.0)),
        TextStyle::new(Color::rgb(0, 0, 0), Dip(18.0)).centered(),
        end,
    ];
    let bands = [
        Rect::new(0, 0, W, 28),
        Rect::new(0, 32, W, 60),
        Rect::new(0, 64, W, 92),
    ];
    surface.with_canvas(Rect::new(0, 0, W, 96), |canvas| {
        for (band, style) in bands.iter().zip(&styles) {
            canvas.draw_text("Hello xui", *band, style);
        }
    });

    let image = surface.to_image();
    let bg = [background.r, background.g, background.b];
    let start = ink_bounds(&image, bands[0], bg).expect("start-aligned ink");
    let center = ink_bounds(&image, bands[1], bg).expect("centred ink");
    let right = ink_bounds(&image, bands[2], bg).expect("end-aligned ink");

    assert!(
        start.0 <= 12,
        "start-aligned ink sits at the left: {start:?}"
    );
    let middle = (center.0 + center.1) / 2;
    assert!(
        (W / 2 - 30..=W / 2 + 30).contains(&middle),
        "centred ink sits near the middle: {center:?}"
    );
    assert!(
        right.1 >= W - 12,
        "end-aligned ink sits at the right: {right:?}"
    );
}

#[test]
fn aligned_empty_text_paints_nothing() {
    let background = Color::rgb(255, 255, 255);
    let mut surface = Surface::new(40, 40);
    surface.fill(background);

    let mut end = TextStyle::new(Color::rgb(0, 0, 0), Dip(12.0));
    end.align = TextAlign::End;
    surface.with_canvas(Rect::new(0, 0, 40, 40), |canvas| {
        canvas.draw_text("", Rect::new(0, 0, 40, 40), &end);
    });

    let bg = [background.r, background.g, background.b, 255];
    assert!(
        surface
            .pixels()
            .as_chunks::<4>()
            .0
            .iter()
            .all(|pixel| *pixel == bg),
        "empty text is a no-op whatever the alignment"
    );
}

#[test]
fn each_line_of_a_multiline_run_is_end_aligned() {
    const W: i32 = 200;
    let background = Color::rgb(255, 255, 255);
    let mut surface = Surface::new(W as u32, 80);
    surface.fill(background);

    let mut end = TextStyle::new(Color::rgb(0, 0, 0), Dip(12.0));
    end.align = TextAlign::End;
    surface.with_canvas(Rect::new(0, 0, W, 80), |canvas| {
        canvas.draw_text("i\nWWWWWWWW\nmm", Rect::new(0, 0, W, 80), &end);
    });

    let image = surface.to_image();
    let bg = [background.r, background.g, background.b];
    // One band per line: 12px text at the default 1.25 line height is 15px.
    let bands = [
        Rect::new(0, 0, W, 15),
        Rect::new(0, 15, W, 30),
        Rect::new(0, 30, W, 45),
    ];
    for band in bands {
        let (left, right) = ink_bounds(&image, band, bg).expect("ink on the line");
        assert!(
            right >= W - 6,
            "the line's right edge sits at the rectangle's right: {band:?} -> {right}"
        );
        assert!(
            left > 20,
            "a short line is pushed right, not drawn from the left: {band:?} -> {left}"
        );
    }
}

#[test]
fn clipped_text_does_not_bleed_outside_the_clip() {
    let mut surface = Surface::new(200, 40);
    surface.fill(Color::rgb(0, 0, 0));
    surface.with_canvas(Rect::new(0, 0, 200, 40), |canvas| {
        canvas.push_clip(Rect::new(0, 0, 50, 40));
        canvas.draw_text(
            "WWWWWWWWWWWW",
            Rect::new(0, 0, 200, 20),
            &TextStyle::new(Color::rgb(255, 255, 255), Dip(14.0)),
        );
        canvas.pop_clip();
    });

    let image = surface.to_image();
    let painted = |x: i32, y: i32| image.pixel(x as u32, y as u32) != Some([0, 0, 0, 255]);
    let outside = (50..200)
        .flat_map(|x| (0..40).map(move |y| (x, y)))
        .filter(|(x, y)| painted(*x, *y))
        .count();
    assert_eq!(outside, 0, "no text pixel lies beyond the clip");
    let inside = (0..50)
        .flat_map(|x| (0..40).map(move |y| (x, y)))
        .filter(|(x, y)| painted(*x, *y))
        .count();
    assert!(inside > 0, "the text still painted inside the clip");
}

#[test]
fn rounded_clipped_text_respects_the_corners() {
    let rect = Rect::new(8, 8, 56, 56);
    let radius = 16.0;
    let style = TextStyle::new(Color::rgb(255, 255, 255), Dip(48.0));
    let draw = |rounded: bool| {
        let mut surface = Surface::new(64, 64);
        surface.fill(Color::rgb(0, 0, 0));
        surface.with_canvas(Rect::new(0, 0, 64, 64), |canvas| {
            if rounded {
                canvas.push_clip_rounded(rect, [Corner::uniform(radius); 4]);
            } else {
                canvas.push_clip(rect);
            }
            canvas.draw_text("MMMM", Rect::new(8, 8, 56, 56), &style);
            canvas.pop_clip();
        });
        surface.to_image()
    };
    let clipped = draw(true);
    let square = draw(false);

    for y in 0..64 {
        for x in 0..64 {
            if clipped.pixel(x, y) == Some([0, 0, 0, 255]) {
                continue;
            }
            assert!(
                inside_rounded(rect, radius, x as i32, y as i32, 1.5),
                "text painted outside the rounded clip at ({x}, {y})"
            );
        }
    }

    // The same text under a plain rectangular clip reaches the corners, so the
    // rounded clip's corner culling above is not vacuous.
    let reaches_corner = (0..64).any(|y| {
        (0..64).any(|x| {
            square.pixel(x, y) != Some([0, 0, 0, 255])
                && !inside_rounded(rect, radius, x as i32, y as i32, -2.0)
        })
    });
    assert!(
        reaches_corner,
        "the text covers a corner, so the rounded clip has something to cut"
    );
}

#[test]
fn a_clipped_image_is_cropped_not_squashed() {
    // A 100x100 image, left half red and right half blue.
    let mut pixels = Vec::new();
    for _ in 0..100 {
        for x in 0..100u32 {
            if x < 50 {
                pixels.extend_from_slice(&[255, 0, 0, 255]);
            } else {
                pixels.extend_from_slice(&[0, 0, 255, 255]);
            }
        }
    }
    let image = Image::from_rgba(100, 100, pixels).expect("image");

    let mut surface = Surface::new(100, 100);
    surface.fill(Color::rgb(0, 0, 0));
    surface.with_canvas(Rect::new(0, 0, 100, 100), |canvas| {
        canvas.push_clip(Rect::new(0, 0, 50, 100));
        canvas.draw_image(&image, Rect::new(0, 0, 100, 100));
        canvas.pop_clip();
    });

    let rendered = surface.to_image();
    assert_eq!(
        rendered.pixel(10, 50),
        Some([255, 0, 0, 255]),
        "the left region stays at the left edge"
    );
    assert_eq!(
        rendered.pixel(40, 50),
        Some([255, 0, 0, 255]),
        "the visible slice is not rescaled"
    );
    assert_eq!(
        rendered.pixel(60, 50),
        Some([0, 0, 0, 255]),
        "the clipped-away half is not squeezed in"
    );
}

/// A rounded clip under a rectangular clip as large as the integer range: the
/// rectangle the mask is built over saturates instead of overflowing.
#[test]
fn a_rounded_clip_under_a_clip_as_big_as_the_integer_range_does_not_overflow() {
    let mut surface = Surface::new(64, 64);
    surface.fill(Color::rgb(0, 0, 0));
    surface.with_canvas(Rect::new(0, 0, 64, 64), |canvas| {
        canvas.push_clip(Rect::new(0, 0, i32::MAX, i32::MAX));
        canvas.push_clip_rounded(Rect::new(4, 4, 60, 60), [Corner::uniform(8.0); 4]);
        canvas.fill_rect(Rect::new(0, 0, 64, 64), Color::rgb(200, 0, 0));
    });
    let image = surface.to_image();
    assert_eq!(
        image.pixel(30, 30),
        Some([200, 0, 0, 255]),
        "inside the clip"
    );
    assert_eq!(image.pixel(1, 1), Some([0, 0, 0, 255]), "outside it");
    assert_eq!(
        image.pixel(4, 4),
        Some([0, 0, 0, 255]),
        "the rounded corner is cut away"
    );
}

/// Lines, outlines and glows are cut by a rectangular clip, as fills are: a
/// stroke is not trimmed like a rectangle (its outline is wider than its path),
/// so it is cut by the clip's mask.
#[test]
fn a_stroke_crossing_a_rectangular_clip_is_cut_by_it() {
    let clip = Rect::new(20, 20, 60, 60);
    let draw = |canvas: &mut crate::SkiaCanvas<'_>| {
        canvas.push_clip(clip);
        canvas.draw_line(
            xui_core::geometry::Point::new(0, 40),
            xui_core::geometry::Point::new(99, 40),
            Color::rgb(255, 0, 0),
            3.0,
        );
        canvas.stroke_ellipse(
            xui_core::geometry::Point::new(40, 40),
            50.0,
            50.0,
            Color::rgb(0, 255, 0),
            3.0,
        );
        canvas.stroke_rect(clip, Color::rgb(0, 0, 255), 4.0);
        canvas.pop_clip();
    };
    let mut surface = Surface::new(100, 100);
    surface.fill(Color::rgb(0, 0, 0));
    surface.with_canvas(Rect::new(0, 0, 100, 100), draw);
    let image = surface.to_image();
    for y in 0..100u32 {
        for x in 0..100u32 {
            let inside = (20..60).contains(&x) && (20..60).contains(&y);
            if !inside {
                assert_eq!(image.pixel(x, y), Some([0, 0, 0, 255]), "({x}, {y})");
            }
        }
    }
    assert_eq!(
        image.pixel(40, 40),
        Some([255, 0, 0, 255]),
        "the line is drawn"
    );
}
