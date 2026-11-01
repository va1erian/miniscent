//! The unscaled image draw behind [`SkiaCanvas::draw_image`]: when an image
//! lands on the surface pixel for pixel, its rows are copied (opaque) or
//! blended source-over (translucent) directly, with no shader pipeline.

use tiny_skia::Pixmap;
use xui_core::geometry::Rect;

/// Draws `src` (premultiplied) with its top-left at `dest`'s, `dest` being
/// exactly `src`'s size, covering only `visible` (a part of `dest`) and the
/// surface. `opaque` says every source pixel has alpha 255.
pub(super) fn unscaled(dst: &mut Pixmap, src: &Pixmap, opaque: bool, dest: Rect, visible: Rect) {
    let (dw, dh) = (dst.width() as i32, dst.height() as i32);
    let left = visible.left.max(0).max(dest.left);
    let top = visible.top.max(0).max(dest.top);
    let right = visible.right.min(dw).min(dest.right);
    let bottom = visible.bottom.min(dh).min(dest.bottom);
    if left >= right || top >= bottom {
        return;
    }
    let width = (right - left) as usize * 4;
    let src_stride = src.width() as usize * 4;
    let dst_stride = dw as usize * 4;
    let src_x = (left - dest.left) as usize * 4;
    let src_data = src.data();
    let dst_data = dst.data_mut();
    for y in top..bottom {
        let s = (y - dest.top) as usize * src_stride + src_x;
        let d = y as usize * dst_stride + left as usize * 4;
        let (from, to) = (&src_data[s..s + width], &mut dst_data[d..d + width]);
        if opaque {
            to.copy_from_slice(from);
        } else {
            over(to, from);
        }
    }
}

/// Premultiplied source-over of one row: `to = from + to * (1 - from.a)`.
fn over(to: &mut [u8], from: &[u8]) {
    for (d, s) in to
        .as_chunks_mut::<4>()
        .0
        .iter_mut()
        .zip(from.as_chunks::<4>().0)
    {
        match s[3] {
            0 => {}
            255 => d.copy_from_slice(s),
            a => {
                let keep = 255 - u32::from(a);
                for (d, s) in d.iter_mut().zip(s) {
                    *d = s.saturating_add(div255(u32::from(*d) * keep));
                }
            }
        }
    }
}

/// `v / 255`, rounded, for `v <= 255 * 255`.
fn div255(v: u32) -> u8 {
    let v = v + 128;
    ((v + (v >> 8)) >> 8) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pixmap(width: u32, height: u32, rgba: [u8; 4]) -> Pixmap {
        let mut p = Pixmap::new(width, height).unwrap();
        for px in p.data_mut().as_chunks_mut::<4>().0 {
            px.copy_from_slice(&rgba);
        }
        p
    }

    #[test]
    fn div255_rounds_like_the_float_division() {
        for v in 0..=255 * 255 {
            assert_eq!(div255(v), (v as f32 / 255.0).round() as u8, "{v}");
        }
    }

    #[test]
    fn an_opaque_image_is_copied_inside_the_clip_only() {
        let mut dst = pixmap(8, 8, [0, 0, 0, 255]);
        let src = pixmap(4, 4, [10, 20, 30, 255]);
        // Placed at (2, 2), clipped to x < 4: two columns land.
        unscaled(
            &mut dst,
            &src,
            true,
            Rect::new(2, 2, 6, 6),
            Rect::new(0, 0, 4, 8),
        );
        let at = |x: usize, y: usize| &dst.data()[(y * 8 + x) * 4..][..4];
        assert_eq!(at(2, 2), &[10, 20, 30, 255]);
        assert_eq!(at(3, 5), &[10, 20, 30, 255]);
        assert_eq!(at(4, 2), &[0, 0, 0, 255]);
        assert_eq!(at(1, 2), &[0, 0, 0, 255]);
        assert_eq!(at(2, 6), &[0, 0, 0, 255]);
    }

    #[test]
    fn an_image_off_the_surface_is_trimmed() {
        let mut dst = pixmap(4, 4, [0, 0, 0, 255]);
        let src = pixmap(4, 4, [9, 9, 9, 255]);
        unscaled(
            &mut dst,
            &src,
            true,
            Rect::new(-2, -3, 2, 1),
            Rect::new(-2, -3, 2, 1),
        );
        assert_eq!(&dst.data()[..4], &[9, 9, 9, 255]);
        assert_eq!(&dst.data()[(4 + 1) * 4..][..4], &[0, 0, 0, 255]);
        unscaled(
            &mut dst,
            &src,
            true,
            Rect::new(10, 10, 14, 14),
            Rect::new(10, 10, 14, 14),
        );
    }

    #[test]
    fn a_translucent_image_blends_source_over() {
        let mut dst = pixmap(1, 1, [200, 100, 0, 255]);
        // 50% white, premultiplied.
        let src = pixmap(1, 1, [128, 128, 128, 128]);
        unscaled(
            &mut dst,
            &src,
            false,
            Rect::new(0, 0, 1, 1),
            Rect::new(0, 0, 1, 1),
        );
        assert_eq!(&dst.data()[..4], &[128 + 100, 128 + 50, 128, 255]);
    }
}
