//! The alpha plane of a cutout texture: one byte per texel with its own mip
//! chain, kept apart from the colour so an opaque texture pays nothing for it
//! (see [`crate::texture::Texture::with_alpha`]).

/// Coverage per texel, 0 (see through) to 255 (solid), and its halvings.
#[derive(Clone, Debug)]
pub(crate) struct AlphaPlane {
    base: Vec<u8>,
    /// Same levels as the colour chain: level `l` is `1 << (l + 1)` times smaller.
    mips: Vec<Vec<u8>>,
}

impl AlphaPlane {
    /// `None` when every texel is solid: such a texture is opaque and stays on
    /// the plain path.
    pub(crate) fn new(base: Vec<u8>) -> Option<Self> {
        base.iter().any(|&a| a < u8::MAX).then(|| Self {
            base,
            mips: Vec::new(),
        })
    }

    /// Build the mip chain for a `w` x `h` plane; a chain that exists is kept.
    pub(crate) fn with_mips(mut self, w: u32, h: u32) -> Self {
        if self.mips.is_empty() {
            self.mips = build(w, h, &self.base);
        }
        self
    }

    /// A sampler over mip `level` (clamped to the chain) of a `w` x `h` texture.
    pub(crate) fn sampler(&self, w: u32, h: u32, level: u32) -> AlphaSampler<'_> {
        match (level as usize).min(self.mips.len()).checked_sub(1) {
            None => AlphaSampler {
                px: &self.base,
                w,
                h,
                shift: 0,
            },
            Some(l) => {
                let shift = l as u32 + 1;
                AlphaSampler {
                    px: &self.mips[l],
                    w: (w >> shift).max(1),
                    h: (h >> shift).max(1),
                    shift,
                }
            }
        }
    }

    /// Alpha of base texel `(x, y)` with wrap-around addressing.
    pub(crate) fn at(&self, w: u32, h: u32, x: i32, y: i32) -> u8 {
        self.sampler(w, h, 0).nearest(x, y)
    }
}

/// One mip level of an alpha plane, at the same full-resolution 16.16
/// coordinates as [`crate::texture::LodSampler`].
#[derive(Clone, Copy)]
pub struct AlphaSampler<'a> {
    px: &'a [u8],
    w: u32,
    h: u32,
    shift: u32,
}

impl AlphaSampler<'_> {
    /// Alpha of the texel under full-resolution texel coordinates `(x, y)`.
    #[inline]
    pub fn nearest(&self, x: i32, y: i32) -> u8 {
        let (xm, ym) = (
            (x as u32 >> self.shift) & (self.w - 1),
            (y as u32 >> self.shift) & (self.h - 1),
        );
        self.fetch(ym as usize * self.w as usize + xm as usize)
    }

    /// Bilinearly filtered alpha, with the same texel centres and wrap as the
    /// colour's bilinear fetch, so the two agree on where an edge is.
    #[inline]
    pub fn bilinear(&self, fu: i32, fv: i32) -> u8 {
        let (fu, fv) = (fu >> self.shift, fv >> self.shift);
        let (u, v) = (fu.wrapping_sub(0x8000), fv.wrapping_sub(0x8000));
        let (x0, y0) = ((u >> 16) as u32, (v >> 16) as u32);
        let (mx, my) = (self.w - 1, self.h - 1);
        let w = self.w as usize;
        let (xa, xb) = ((x0 & mx) as usize, (x0.wrapping_add(1) & mx) as usize);
        let (ra, rb) = (
            (y0 & my) as usize * w,
            (y0.wrapping_add(1) & my) as usize * w,
        );
        let (tx, ty) = (((u >> 8) & 0xff) as u32, ((v >> 8) & 0xff) as u32);
        let mix = |a: u32, b: u32, t: u32| (a * (256 - t) + b * t) >> 8;
        let at = |i: usize| u32::from(self.fetch(i));
        let top = mix(at(ra + xa), at(ra + xb), tx);
        let bottom = mix(at(rb + xa), at(rb + xb), tx);
        mix(top, bottom, ty) as u8
    }

    /// A short plane (it cannot be, the indices are masked) reads as clear.
    #[inline]
    fn fetch(&self, i: usize) -> u8 {
        self.px.get(i).copied().unwrap_or(0)
    }
}

/// Levels below the base, halving each side down to 1x1 like [`crate::mips::build`].
fn build(w: u32, h: u32, base: &[u8]) -> Vec<Vec<u8>> {
    let mut levels: Vec<Vec<u8>> = Vec::new();
    let (mut cw, mut ch) = (w, h);
    while cw > 1 || ch > 1 {
        let (nw, nh) = ((cw / 2).max(1), (ch / 2).max(1));
        let prev: &[u8] = levels.last().map_or(base, Vec::as_slice);
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
fn box4(src: &[u8], w: u32, h: u32, x: u32, y: u32) -> u8 {
    let mut sum = 0u32;
    for dy in 0..2 {
        for dx in 0..2 {
            let (sx, sy) = ((x * 2 + dx).min(w - 1), (y * 2 + dy).min(h - 1));
            sum += u32::from(
                src.get(sy as usize * w as usize + sx as usize)
                    .copied()
                    .unwrap_or(0),
            );
        }
    }
    (sum / 4) as u8
}

#[cfg(test)]
#[path = "alpha_tests.rs"]
mod alpha_tests;
