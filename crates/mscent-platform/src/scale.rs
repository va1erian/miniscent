//! Presentation of a small framebuffer into a window.
//!
//! The image fills the window at its own aspect ratio (bars on the other
//! axis). An exact whole-number magnification replicates pixels, so a
//! full-resolution frame stays crisp; any other ratio is filtered bilinearly,
//! so the adaptive resolution never shows stair-steps of uneven width.

/// Where the image lands in the window: `(x, y, width, height)`. The largest
/// rectangle of the image's aspect ratio that fits, centred; the rest of the
/// window is letterbox.
pub fn fit_rect(src_w: u32, src_h: u32, win_w: u32, win_h: u32) -> (u32, u32, u32, u32) {
    if src_w == 0 || src_h == 0 || win_w == 0 || win_h == 0 {
        return (0, 0, 0, 0);
    }
    let (sw, sh) = (u64::from(src_w), u64::from(src_h));
    let (ww, wh) = (u64::from(win_w), u64::from(win_h));
    let scaled =
        |side: u64, num: u64, den: u64, max: u64| ((side * num + den / 2) / den).clamp(1, max);
    // Compare aspect ratios without division: the window is narrower than
    // the image when win_w / win_h < src_w / src_h.
    let (w, h) = if ww * sh <= wh * sw {
        (ww, scaled(sh, ww, sw, wh))
    } else {
        (scaled(sw, wh, sh, ww), wh)
    };
    let (w, h) = (w as u32, h as u32);
    ((win_w - w) / 2, (win_h - h) / 2, w, h)
}

/// Fills `dst` (`win_w` x `win_h`) with black bars and draws `src` (`src_w` x
/// `src_h`) in the [`fit_rect`] rectangle. Does nothing when a slice does not
/// match its dimensions.
pub fn blit_fit(src: &[u32], src_w: u32, src_h: u32, dst: &mut [u32], win_w: u32, win_h: u32) {
    let area = |w: u32, h: u32| (w as usize).checked_mul(h as usize);
    if area(src_w, src_h) != Some(src.len()) || area(win_w, win_h) != Some(dst.len()) {
        return;
    }
    let (x0, y0, w, h) = fit_rect(src_w, src_h, win_w, win_h);
    let ww = win_w as usize;
    let (x0, y0, w, h) = (x0 as usize, y0 as usize, w as usize, h as usize);
    // Black out the bars only; the image overwrites the rest.
    dst[..y0 * ww].fill(0);
    dst[(y0 + h) * ww..].fill(0);
    if w == 0 || h == 0 {
        return;
    }
    let whole = w % src_w as usize == 0
        && h % src_h as usize == 0
        && w / src_w as usize == h / src_h as usize;
    // The image rows, with the window's stride; the bars on each row are
    // cleared by the drawing functions.
    let img = &mut dst[y0 * ww..(y0 + h) * ww];
    let fit = Fit {
        src_w: src_w as usize,
        src_h: src_h as usize,
        stride: ww,
        x0,
        w,
        h,
    };
    if whole {
        replicate(src, img, &fit);
    } else {
        smooth(src, img, &fit);
    }
}

/// Source and destination geometry of one blit.
struct Fit {
    src_w: usize,
    src_h: usize,
    /// Window width, the stride of the destination rows.
    stride: usize,
    x0: usize,
    w: usize,
    h: usize,
}

/// Pixel replication by a whole factor (`w / src_w`).
fn replicate(src: &[u32], img: &mut [u32], f: &Fit) {
    let factor = f.w / f.src_w;
    let mut last_row: Option<usize> = None;
    for dy in 0..f.h {
        let sy = (dy / factor).min(f.src_h - 1);
        let start = dy * f.stride;
        let row = &mut img[start..start + f.stride];
        row[..f.x0].fill(0);
        row[f.x0 + f.w..].fill(0);
        if last_row == Some(sy) {
            // Same source row as the line above: a copy, no resampling.
            img.copy_within(
                start - f.stride + f.x0..start - f.stride + f.x0 + f.w,
                start + f.x0,
            );
        } else {
            let line = &src[sy * f.src_w..(sy + 1) * f.src_w];
            for (px, s) in row[f.x0..f.x0 + f.w].chunks_exact_mut(factor).zip(line) {
                px.fill(*s);
            }
        }
        last_row = Some(sy);
    }
}

/// Blend `a` toward `b` by `t / 256` per channel, two channels per multiply.
#[inline(always)]
fn lerp(a: u32, b: u32, t: u32) -> u32 {
    let (rb, ag) = (0x00ff_00ff, 0x00ff_00ff);
    let lo = ((a & rb) * (256 - t) + (b & rb) * t) >> 8 & rb;
    let hi = (((a >> 8) & ag) * (256 - t) + ((b >> 8) & ag) * t) & !rb;
    lo | hi
}

/// Source position of destination pixel `d` of `dst` pixels over `src`
/// pixels (centres aligned): the lower index and the weight of the upper one
/// out of 256, clamped to the image.
#[inline(always)]
fn tap(d: usize, src: usize, dst: usize) -> (usize, usize, u32) {
    let x = ((d as f32 + 0.5) * src as f32 / dst as f32 - 0.5).clamp(0.0, (src - 1) as f32);
    let i = x as usize;
    (i, (i + 1).min(src - 1), ((x - i as f32) * 256.0) as u32)
}

/// Bilinear magnification: each needed source row is resampled to the
/// window width once, then neighbouring rows are blended per output row.
fn smooth(src: &[u32], img: &mut [u32], f: &Fit) {
    let taps: Vec<_> = (0..f.w).map(|dx| tap(dx, f.src_w, f.w)).collect();
    let mut cache: [(Option<usize>, Vec<u32>); 2] = [(None, vec![0; f.w]), (None, vec![0; f.w])];
    for dy in 0..f.h {
        let (y0, y1, fy) = tap(dy, f.src_h, f.h);
        let a = slot(&mut cache, y0, None, (src, f.src_w), &taps);
        let b = slot(&mut cache, y1, Some(a), (src, f.src_w), &taps);
        let start = dy * f.stride;
        let row = &mut img[start..start + f.stride];
        row[..f.x0].fill(0);
        row[f.x0 + f.w..].fill(0);
        for ((o, &p), &q) in row[f.x0..f.x0 + f.w]
            .iter_mut()
            .zip(&cache[a].1)
            .zip(&cache[b].1)
        {
            *o = lerp(p, q, fy);
        }
    }
}

/// Index of the cache entry holding source row `y` resampled to `w` pixels,
/// filling one (never `keep`) if none does. Rows are asked for in rising
/// order, so the older entry is the one to replace.
fn slot(
    cache: &mut [(Option<usize>, Vec<u32>); 2],
    y: usize,
    keep: Option<usize>,
    (src, sw): (&[u32], usize),
    taps: &[(usize, usize, u32)],
) -> usize {
    if let Some(i) = cache.iter().position(|(row, _)| *row == Some(y)) {
        return i;
    }
    let evict = match keep {
        Some(k) => 1 - k,
        None => usize::from(cache[1].0 < cache[0].0),
    };
    let line = &src[y * sw..(y + 1) * sw];
    for (o, &(i, j, t)) in cache[evict].1.iter_mut().zip(taps) {
        *o = lerp(line[i], line[j], t);
    }
    cache[evict].0 = Some(y);
    evict
}

#[cfg(test)]
#[path = "scale_tests.rs"]
mod tests;
