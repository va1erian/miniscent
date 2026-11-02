//! The kernels of the half-res layer: laying it over the frame, and taking the
//! nearest depth of each 2x2 block. Each has a scalar definition, and SSE2 (and
//! for the composite AVX2) paths that are bit-identical to it.
//!
//! A layer pixel covers two frame pixels across. With `keep = alpha + 1` (1 to
//! 256, so an untouched layer pixel, alpha 255, leaves the frame exactly as it
//! was) the definition per channel is
//!
//! ```text
//! dst = min(255, layer + dst * keep / 256)
//! ```
//!
//! and the pixel's alpha byte is left as it was. A layer pixel that nothing
//! touched (colour 0, alpha 255, [`CLEAR`]) therefore leaves the frame pixel
//! exactly as it was, which the vector paths use to skip a whole vector of
//! them. `dst * keep` is at most `255 * 256`, so the vector paths work on
//! 16-bit lanes as the sprite kernels do.

// The crate denies unsafe code; this module is an exception, like `kernels`:
// each block is justified by a run-time feature check or the baseline.
#![allow(unsafe_code)]

use crate::simd::{Level, usable};

#[cfg(target_arch = "x86_64")]
#[path = "layer_kernels_x86.rs"]
mod x86;

/// Colour and keep of a layer pixel nothing was drawn into.
pub(super) const CLEAR: u32 = 0xff00_0000;

/// Composite `layer` over `dst`, pixel `x` of `dst` using `layer[x / 2]`. A
/// `layer` too short for `dst` leaves the rest of `dst` alone. An unsupported
/// level runs the scalar code.
pub(super) fn composite_row(level: Level, dst: &mut [u32], layer: &[u32]) {
    let n = dst.len().min(2 * layer.len());
    let dst = &mut dst[..n];
    match usable(level) {
        #[cfg(target_arch = "x86_64")]
        // SAFETY: `usable` only returns Avx2 after run-time detection.
        Level::Avx2 => unsafe { x86::composite_avx2(dst, layer) },
        #[cfg(target_arch = "x86_64")]
        // SAFETY: SSE2 is part of the x86-64 baseline.
        Level::Sse2 => unsafe { x86::composite_sse2(dst, layer) },
        _ => composite_scalar(dst, layer),
    }
}

/// One frame pixel under one layer pixel.
pub(super) fn composite_px(dst: u32, layer: u32) -> u32 {
    let keep = (layer >> 24) + 1;
    let ch = |shift: u32| {
        (((layer >> shift) & 255) + ((((dst >> shift) & 255) * keep) >> 8)).min(255) << shift
    };
    (dst & 0xff00_0000) | ch(16) | ch(8) | ch(0)
}

pub(super) fn composite_scalar(dst: &mut [u32], layer: &[u32]) {
    for (pair, &l) in dst.chunks_mut(2).zip(layer) {
        for d in pair {
            *d = composite_px(*d, l);
        }
    }
}

/// `out[i]` = the nearest (largest `1/z`) of the pixels `2i` and `2i + 1` of two
/// rows. An odd last pixel stands for a block of one. A NaN is not a case (the
/// depth buffer holds none) but is defined: `nearer` is `max`'s own rule, which
/// returns its second argument unless the first is greater.
pub(super) fn nearest_row(level: Level, out: &mut [f32], top: &[f32], bottom: &[f32]) {
    match usable(level) {
        #[cfg(target_arch = "x86_64")]
        // SAFETY: SSE2 is part of the x86-64 baseline.
        Level::Sse2 | Level::Avx2 => unsafe { x86::nearest_sse2(out, top, bottom) },
        _ => nearest_scalar(out, top, bottom),
    }
}

fn nearer(a: f32, b: f32) -> f32 {
    if a > b { a } else { b }
}

pub(super) fn nearest_scalar(out: &mut [f32], top: &[f32], bottom: &[f32]) {
    for (o, (t, b)) in out.iter_mut().zip(top.chunks(2).zip(bottom.chunks(2))) {
        *o = match (t, b) {
            ([t0, t1], [b0, b1]) => nearer(nearer(*t0, *b0), nearer(*t1, *b1)),
            ([t0, ..], [b0, ..]) => nearer(*t0, *b0),
            _ => *o,
        };
    }
}

#[cfg(test)]
#[path = "layer_kernels_tests.rs"]
mod layer_kernels_tests;
