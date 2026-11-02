//! The sprite masks: `u8` coverage generated from a fixed seed at start-up, so
//! there are no asset files and no decoder to attack.
//!
//! Fourteen masks (a soft disc, four noisy puffs, a ring and an eight-frame
//! fireball) are drawn at 128x128 and box-filtered down to 64, 32, 16 and 8.
//! That is `14 * 21824` bytes, about 300 KB.
//!
//! The generator uses only `+ - * /` and `sqrt`, which IEEE 754 fixes exactly,
//! so the bank is the same on every machine and tests can pin it.

use mscent_math::rng::Rng;

use super::Mask;

/// Side of the smallest and the largest mip.
const MIN_SIDE: usize = 8;
const MAX_SIDE: usize = 128;
/// Number of mips: 8, 16, 32, 64, 128.
pub(super) const MIPS: usize = 5;
const MASKS: usize = 14;
/// Bytes of one mask's whole mip chain.
const MASK_BYTES: usize = 64 + 256 + 1024 + 4096 + 16384;
/// Where each mip starts inside a mask's chain, smallest first.
const MIP_START: [usize; MIPS] = [0, 64, 320, 1344, 5440];
const SEED: u64 = 0x6d73_6365_6e74_f001;

/// All generated masks and their mips.
#[derive(Clone, Debug)]
pub struct SpriteBank {
    data: Vec<u8>,
}

impl Default for SpriteBank {
    fn default() -> Self {
        Self::new()
    }
}

impl SpriteBank {
    /// Generate the bank. Deterministic: every call gives the same bytes.
    pub fn new() -> Self {
        let mut data = Vec::with_capacity(MASKS * MASK_BYTES);
        push_mips(
            &mut data,
            shape(|d2, _| {
                let w = (1.0 - d2).max(0.0);
                w * w
            }),
        );
        for k in 0..4 {
            let noise = Noise::new(SEED + 1 + k);
            push_mips(
                &mut data,
                shape(|d2, p| {
                    smooth(((1.0 - d2).max(0.0) * (0.3 + 1.2 * noise.at(p, 0.0))).min(1.0))
                }),
            );
        }
        push_mips(&mut data, shape(|d2, _| ring(d2.sqrt())));
        let noise = Noise::new(SEED + 9);
        for frame in 0..8 {
            let t = frame as f32 / 7.0;
            push_mips(&mut data, shape(|d2, p| fire(d2, noise.at(p, t), t)));
        }
        debug_assert_eq!(data.len(), MASKS * MASK_BYTES);
        Self { data }
    }

    /// Mip `level` (clamped to the largest) of a mask, row-major, and its side.
    /// Empty only if the bank were malformed, which `new` cannot produce.
    pub(super) fn mip(&self, mask: Mask, level: usize) -> (&[u8], usize) {
        let level = level.min(MIPS - 1);
        let side = MIN_SIDE << level;
        let start = mask.index() * MASK_BYTES + MIP_START[level];
        let bytes = self.data.get(start..start + side * side);
        (bytes.unwrap_or_default(), side)
    }
}

impl Mask {
    /// Position in the bank. Variant numbers wrap, so any `u8` is valid.
    pub(super) fn index(self) -> usize {
        match self {
            Mask::Soft => 0,
            Mask::Puff(i) => 1 + usize::from(i) % 4,
            Mask::Ring => 5,
            Mask::Fire(i) => 6 + usize::from(i) % 8,
        }
    }
}

fn smooth(x: f32) -> f32 {
    x * x * (3.0 - 2.0 * x)
}

/// A band of coverage around radius 0.7, zero outside 0.48..0.92.
fn ring(d: f32) -> f32 {
    let x = (d - 0.7) / 0.22;
    let t = (1.0 - x * x).max(0.0);
    t * t
}

/// A fireball at time `t` in 0..=1: it grows and thins as it cools.
fn fire(d2: f32, noise: f32, t: f32) -> f32 {
    let radius = 0.35 + 0.6 * t;
    let w = (1.0 - d2 / (radius * radius)).max(0.0);
    (w * (0.55 + 0.9 * noise) * (1.0 - 0.75 * t)).min(1.0)
}

/// Render a 128x128 mask from a function of the squared distance from the
/// centre (1.0 at the edge midpoints) and the position in 0..1.
fn shape(f: impl Fn(f32, (f32, f32)) -> f32) -> Vec<u8> {
    let mut out = Vec::with_capacity(MAX_SIDE * MAX_SIDE);
    for j in 0..MAX_SIDE {
        for i in 0..MAX_SIDE {
            let p = (
                (i as f32 + 0.5) / MAX_SIDE as f32,
                (j as f32 + 0.5) / MAX_SIDE as f32,
            );
            let (cx, cy) = (2.0 * p.0 - 1.0, 2.0 * p.1 - 1.0);
            let v = f(cx * cx + cy * cy, p).clamp(0.0, 1.0);
            out.push((v * 255.0 + 0.5) as u8);
        }
    }
    out
}

/// Append the mip chain of a 128x128 mask, smallest first.
fn push_mips(out: &mut Vec<u8>, base: Vec<u8>) {
    let mut chain = vec![base];
    let mut side = MAX_SIDE;
    while side > MIN_SIDE {
        chain.push(halve(&chain[chain.len() - 1], side));
        side /= 2;
    }
    for level in chain.iter().rev() {
        out.extend_from_slice(level);
    }
}

/// 2x2 box filter, rounding to nearest.
fn halve(src: &[u8], side: usize) -> Vec<u8> {
    let half = side / 2;
    let at = |x: usize, y: usize| u32::from(src[y * side + x]);
    let mut out = Vec::with_capacity(half * half);
    for y in 0..half {
        for x in 0..half {
            let sum = at(2 * x, 2 * y) + at(2 * x + 1, 2 * y) + at(2 * x, 2 * y + 1);
            out.push(((sum + at(2 * x + 1, 2 * y + 1) + 2) >> 2) as u8);
        }
    }
    out
}

/// Two octaves of smooth value noise on seeded lattices, in 0..=1.
struct Noise {
    coarse: [f32; 25],
    fine: [f32; 81],
}

impl Noise {
    fn new(seed: u64) -> Self {
        let mut rng = Rng::new(seed);
        Self {
            coarse: std::array::from_fn(|_| rng.range_f32(0.0, 1.0)),
            fine: std::array::from_fn(|_| rng.range_f32(0.0, 1.0)),
        }
    }

    /// Noise at `p`, drifted by `t` (0..=1) so successive frames differ
    /// without being unrelated.
    fn at(&self, p: (f32, f32), t: f32) -> f32 {
        let (u, v) = (p.0, 0.15 * t + 0.85 * p.1);
        0.65 * lattice(&self.coarse, 5, u, v) + 0.35 * lattice(&self.fine, 9, u, v)
    }
}

/// Bilinear (smoothstep-weighted) sample of an `n`x`n` lattice at `u`, `v` in
/// 0..=1.
fn lattice(grid: &[f32], n: usize, u: f32, v: f32) -> f32 {
    let (x, y) = (u * (n - 1) as f32, v * (n - 1) as f32);
    let (ix, iy) = ((x as usize).min(n - 2), (y as usize).min(n - 2));
    let (fx, fy) = (smooth(x - ix as f32), smooth(y - iy as f32));
    let g = |i: usize, j: usize| grid[j * n + i];
    let top = g(ix, iy) + (g(ix + 1, iy) - g(ix, iy)) * fx;
    let bottom = g(ix, iy + 1) + (g(ix + 1, iy + 1) - g(ix, iy + 1)) * fx;
    top + (bottom - top) * fy
}

#[cfg(test)]
#[path = "bank_tests.rs"]
mod bank_tests;
