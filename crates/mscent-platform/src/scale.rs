//! Nearest-neighbour presentation of a small framebuffer into a window.

/// Where the image lands in the window: `(x, y, width, height)`. The scale
/// is the largest integer that fits (pixel-exact); only when the window is
/// smaller than the image does it shrink to fit, keeping the aspect ratio.
/// The rest of the window is letterbox.
pub fn fit_rect(src_w: u32, src_h: u32, win_w: u32, win_h: u32) -> (u32, u32, u32, u32) {
    if src_w == 0 || src_h == 0 || win_w == 0 || win_h == 0 {
        return (0, 0, 0, 0);
    }
    let int = (win_w / src_w).min(win_h / src_h);
    let (w, h) = if int >= 1 {
        (src_w * int, src_h * int)
    } else {
        let s = (f64::from(win_w) / f64::from(src_w)).min(f64::from(win_h) / f64::from(src_h));
        (
            ((f64::from(src_w) * s) as u32).clamp(1, win_w),
            ((f64::from(src_h) * s) as u32).clamp(1, win_h),
        )
    };
    ((win_w - w) / 2, (win_h - h) / 2, w, h)
}

/// Fills `dst` (`win_w` x `win_h`) with black and draws `src` (`src_w` x
/// `src_h`) centred at the [`fit_rect`] size. Does nothing when a slice does
/// not match its dimensions.
pub fn blit_fit(src: &[u32], src_w: u32, src_h: u32, dst: &mut [u32], win_w: u32, win_h: u32) {
    let area = |w: u32, h: u32| (w as usize).checked_mul(h as usize);
    if area(src_w, src_h) != Some(src.len()) || area(win_w, win_h) != Some(dst.len()) {
        return;
    }
    dst.fill(0);
    let (x0, y0, w, h) = fit_rect(src_w, src_h, win_w, win_h);
    if w == 0 || h == 0 {
        return;
    }
    let (sw, sh, ww) = (src_w as usize, src_h as usize, win_w as usize);
    let (w, h) = (w as usize, h as usize);
    let mut last_row: Option<usize> = None;
    for dy in 0..h {
        let sy = dy * sh / h;
        let start = (y0 as usize + dy) * ww + x0 as usize;
        if last_row == Some(sy) {
            // Same source row as the line above: copy it instead of resampling.
            let prev = start - ww;
            dst.copy_within(prev..prev + w, start);
            continue;
        }
        let line = &src[sy * sw..(sy + 1) * sw];
        for (dx, px) in dst[start..start + w].iter_mut().enumerate() {
            *px = line[dx * sw / w];
        }
        last_row = Some(sy);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integer_scale_is_centred_with_letterbox() {
        assert_eq!(fit_rect(4, 3, 14, 10), (1, 0, 12, 9));
        assert_eq!(fit_rect(4, 3, 4, 3), (0, 0, 4, 3));
    }

    #[test]
    fn smaller_window_shrinks_keeping_aspect() {
        let (x, y, w, h) = fit_rect(8, 4, 4, 4);
        assert_eq!((w, h), (4, 2));
        assert_eq!((x, y), (0, 1));
        assert_eq!(fit_rect(0, 4, 4, 4), (0, 0, 0, 0));
    }

    #[test]
    fn blit_replicates_pixels_and_blacks_the_bars() {
        let src = [1, 2, 3, 4];
        let mut dst = [9u32; 5 * 4];
        blit_fit(&src, 2, 2, &mut dst, 5, 4);
        // 2x scale in a 5x4 window: a 4x4 image at x = 0 (offset (5-4)/2 = 0).
        assert_eq!(&dst[0..5], &[1, 1, 2, 2, 0]);
        assert_eq!(&dst[5..10], &[1, 1, 2, 2, 0]);
        assert_eq!(&dst[10..15], &[3, 3, 4, 4, 0]);
        assert_eq!(&dst[15..20], &[3, 3, 4, 4, 0]);
    }

    #[test]
    fn mismatched_slices_are_ignored() {
        let mut dst = [7u32; 4];
        blit_fit(&[1, 2, 3], 2, 2, &mut dst, 2, 2);
        assert_eq!(dst, [7; 4]);
    }
}
