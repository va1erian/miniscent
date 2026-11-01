#![forbid(unsafe_code)]

//! The layout overlay a [`Snapshot`](super::Snapshot) can draw: a one-pixel
//! outline around every rectangle the window's layouts placed.

use xui_core::Rect;

/// The outline colour: magenta, which no theme uses.
const OUTLINE: [u8; 4] = [255, 0, 255, 255];

/// Draws the edges of each of `rects` into top-down RGBA `pixels` of a
/// `width` x `height` image, clipped to the image.
pub(super) fn outline(pixels: &mut [u8], width: u32, height: u32, rects: &[Rect]) {
    let (width, height) = (width as i32, height as i32);
    let mut plot = |x: i32, y: i32| {
        let at = ((y * width + x) * 4) as usize;
        pixels[at..at + 4].copy_from_slice(&OUTLINE);
    };
    for rect in rects.iter().filter(|rect| !rect.is_empty()) {
        let (right, bottom) = (rect.right - 1, rect.bottom - 1);
        // Walk only the part of each edge inside the image, so a rectangle
        // placed far off screen costs nothing; an edge outside the image is
        // not drawn at all.
        let xs = rect.left.max(0)..=right.min(width - 1);
        let ys = rect.top.max(0)..=bottom.min(height - 1);
        for y in [rect.top, bottom] {
            if (0..height).contains(&y) {
                xs.clone().for_each(|x| plot(x, y));
            }
        }
        for x in [rect.left, right] {
            if (0..width).contains(&x) {
                ys.clone().for_each(|y| plot(x, y));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn outlines_only_the_edges_and_clips_to_the_image() {
        let mut pixels = vec![0u8; 4 * 4 * 4];
        outline(
            &mut pixels,
            4,
            4,
            &[Rect::new(1, 1, 3, 3), Rect::new(-5, -5, 1, 1)],
        );
        let at = |x: usize, y: usize| pixels[(y * 4 + x) * 4];
        assert_eq!(at(1, 1), 255);
        assert_eq!(at(2, 2), 255, "a 2x2 rect is all edge");
        assert_eq!(at(3, 3), 0);
        assert_eq!(at(0, 0), 255, "the clipped rect's corner");
    }

    #[test]
    fn a_rectangle_far_off_screen_is_cheap_and_draws_only_its_visible_edges() {
        let mut pixels = vec![0u8; 4 * 4 * 4];
        let huge = Rect::new(-100_000_000, 1, 100_000_000, 100_000_000);
        outline(&mut pixels, 4, 4, &[huge]);
        let at = |x: usize, y: usize| pixels[(y * 4 + x) * 4];
        assert_eq!(at(0, 1), 255, "the top edge crosses the image");
        assert_eq!(at(0, 2), 0, "the left edge is far off screen");
        assert_eq!(at(3, 3), 0, "so is the bottom one");
    }
}
