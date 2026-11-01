//! Post-filter on the direct light of one face's luxel grid. Hard shadow
//! edges, even area-sampled, leave luxel-scale steps that bilinear
//! reconstruction turns into a sawtooth. A small separable tent removes
//! them. Colour and the luminance-weighted (not yet normalised) direction
//! vector are filtered together, so direction follows intensity and never
//! flips at a shadow edge. It runs inside one face's rectangle only (edges
//! clamp), so nothing bleeds between faces, and before ambient or albedo
//! are involved.

use super::Luxel;
use mscent_math::Vec3;

/// Weight of the centre tap; each neighbour gets `(1 - CENTRE) / 2`. A
/// plain `[1 2 1] / 4` tent is 0.5; 0.4 is a little wider, which the
/// crawl metric prefers, and still crisp at 0.125 world units per luxel.
const CENTRE: f32 = 0.4;

/// Smooths a `w` by `h` grid in place (rows then columns).
pub(super) fn smooth(grid: &mut [Luxel], w: usize, h: usize) {
    if w == 0 || h == 0 || grid.len() != w * h {
        return;
    }
    let mut tmp = grid.to_vec();
    pass(grid, &mut tmp, w, h, 1);
    pass(&tmp, grid, w, h, w);
}

/// One 1-D tent pass. `step` moves along the filtered axis, `lines` counts
/// the other axis; the `n` taps per line are read from `src`, written to `dst`.
fn pass(src: &[Luxel], dst: &mut [Luxel], w: usize, h: usize, step: usize) {
    let (n, lines, line_step) = if step == 1 { (w, h, w) } else { (h, w, 1) };
    let side = (1.0 - CENTRE) * 0.5;
    for line in 0..lines {
        let base = line * line_step;
        for i in 0..n {
            let at = |k: usize| src[base + k * step];
            let (c, lo, hi) = (at(i), at(i.saturating_sub(1)), at((i + 1).min(n - 1)));
            dst[base + i * step] = Luxel {
                color: mix(c.color, lo.color, hi.color, side),
                dir: mix(c.dir, lo.dir, hi.dir, side),
            };
        }
    }
}

fn mix(c: Vec3, lo: Vec3, hi: Vec3, side: f32) -> Vec3 {
    c * CENTRE + lo * side + hi * side
}
