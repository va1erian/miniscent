//! Albedo mip chain: box-filtered halvings used to kill shimmer on surfaces
//! seen at a distance or at a grazing angle.
//!
//! Only the albedo is filtered. Normal maps stay at full size and are
//! fetched with the full-resolution texel coordinates.

/// Levels below the base (level 1 is half size) for a `w` x `h` texture.
pub fn build(w: u32, h: u32, base: &[u32]) -> Vec<Vec<u32>> {
    let mut levels: Vec<Vec<u32>> = Vec::new();
    let (mut cw, mut ch) = (w, h);
    while cw > 1 || ch > 1 {
        let (nw, nh) = ((cw / 2).max(1), (ch / 2).max(1));
        let prev: &[u32] = levels.last().map_or(base, Vec::as_slice);
        let mut next = Vec::with_capacity(nw as usize * nh as usize);
        for y in 0..nh {
            for x in 0..nw {
                next.push(box4(prev, cw, ch, x, y));
            }
        }
        levels.push(next);
        (cw, ch) = (nw, nh);
    }
    levels
}

/// Average of the up-to-2x2 source texels under destination texel `(x, y)`.
fn box4(src: &[u32], w: u32, h: u32, x: u32, y: u32) -> u32 {
    let mut sum = [0u32; 3];
    let mut n = 0;
    for dy in 0..2 {
        for dx in 0..2 {
            let (sx, sy) = ((x * 2 + dx).min(w - 1), (y * 2 + dy).min(h - 1));
            let t = src.get(sy as usize * w as usize + sx as usize).copied();
            let t = t.unwrap_or(0);
            for (k, s) in sum.iter_mut().enumerate() {
                *s += (t >> (16 - 8 * k)) & 0xff;
            }
            n += 1;
        }
    }
    0xff00_0000 | (sum[0] / n) << 16 | (sum[1] / n) << 8 | (sum[2] / n)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chain_halves_down_to_one_texel_and_averages() {
        let base = vec![0xff00_0000, 0xffff_ffff, 0xffff_ffff, 0xff00_0000];
        let m = build(2, 2, &base);
        assert_eq!(m.len(), 1);
        assert_eq!(m[0], vec![0xff7f_7f7f]);
    }

    #[test]
    fn rectangular_textures_keep_halving_the_long_side() {
        let m = build(4, 1, &[0xff00_0000; 4]);
        assert_eq!(m.len(), 2);
        assert_eq!(m[1].len(), 1);
    }
}
