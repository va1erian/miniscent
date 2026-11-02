//! Row kernels that blend one sprite row into the framebuffer: a scalar
//! definition, and SSE2 and AVX2 paths that are bit-identical to it.
//!
//! A row is a run of destination pixels, the stored `1/z` under each, and one
//! coverage byte per pixel. Tint and sprite depth are constants of the call, so
//! the depth test is a single vector compare. A pixel is touched only when
//! `inv_z >= depth` (false for a NaN depth); a touched pixel gets alpha 255, an
//! untouched one is left exactly as it was. Drawing into the half-res layer
//! ([`Dest::Layer`]) is the same arithmetic with the alpha byte kept as a
//! "survives" factor instead of forced.
//!
//! The definitions, with `m` the coverage and `s = m * tint_channel / 256`:
//!
//! ```text
//! add:  dst = min(255, dst + s)
//! over: dst = min(255, s + dst * (256 - m * tint_alpha / 256) / 256)
//! ```
//!
//! In the layer the same `over` formula runs on the alpha byte too, with no
//! source (`s = 0`), and `add` leaves it. So after any number of sprites the
//! byte is the product of their keep factors, rounded down at each step.
//!
//! Every division is an integer shift (round down). All products stay below
//! 65536 (`255 * 255`, and `255 * 256` for `dst * k`), so the vector paths work
//! on 16-bit lanes. The `min` only matters for a tint that is not truly
//! premultiplied (a colour channel above its alpha).

// The crate denies unsafe code; this module is an exception. The unsafe is in
// the x86 module and in the dispatch below, where each block is justified by a
// run-time feature check or the baseline.
#![allow(unsafe_code)]

use super::Blend;
use crate::simd::{Level, usable};

#[cfg(target_arch = "x86_64")]
#[path = "kernels_x86.rs"]
mod x86;

/// Where a row is blended. The two differ only in the alpha byte of the
/// destination pixel, which is the whole difference between drawing into the
/// frame and into the half-res layer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Dest {
    /// A scene pixel: a touched pixel gets alpha 255.
    Frame,
    /// A layer pixel: colour accumulates in RGB and alpha holds how much of
    /// what lies under the layer survives (255 for all of it, from a fresh
    /// layer). "Over" multiplies it by the sprite's own keep factor; "add"
    /// leaves it. The sprite's own alpha is never stored.
    Layer,
}

/// Blend one row, the way `how` says. All three slices are cut to the
/// shortest length. An unsupported level runs the scalar code.
pub(super) fn blend_row(
    level: Level,
    (blend, dest): (Blend, Dest),
    dst: &mut [u32],
    depth: &[f32],
    mask: &[u8],
    tint: u32,
    inv_z: f32,
) {
    let n = dst.len().min(depth.len()).min(mask.len());
    let (dst, depth, mask) = (&mut dst[..n], &depth[..n], &mask[..n]);
    match dest {
        Dest::Frame => blend_in::<false>(level, blend, dst, depth, mask, tint, inv_z),
        Dest::Layer => blend_in::<true>(level, blend, dst, depth, mask, tint, inv_z),
    }
}

/// [`blend_row`] on equal-length slices; `LAYER` is [`Dest::Layer`].
fn blend_in<const LAYER: bool>(
    level: Level,
    blend: Blend,
    dst: &mut [u32],
    depth: &[f32],
    mask: &[u8],
    tint: u32,
    inv_z: f32,
) {
    match (usable(level), blend) {
        #[cfg(target_arch = "x86_64")]
        // SAFETY: `usable` only returns Avx2 after run-time detection; the
        // three slices have equal length.
        (Level::Avx2, Blend::Add) => unsafe {
            x86::add_avx2::<LAYER>(dst, depth, mask, tint, inv_z)
        },
        #[cfg(target_arch = "x86_64")]
        // SAFETY: as above.
        (Level::Avx2, Blend::Over) => unsafe {
            x86::over_avx2::<LAYER>(dst, depth, mask, tint, inv_z)
        },
        #[cfg(target_arch = "x86_64")]
        // SAFETY: SSE2 is part of the x86-64 baseline; equal lengths as above.
        (Level::Sse2, Blend::Add) => unsafe {
            x86::add_sse2::<LAYER>(dst, depth, mask, tint, inv_z)
        },
        #[cfg(target_arch = "x86_64")]
        // SAFETY: as above.
        (Level::Sse2, Blend::Over) => unsafe {
            x86::over_sse2::<LAYER>(dst, depth, mask, tint, inv_z)
        },
        (_, Blend::Add) => add_scalar::<LAYER>(dst, depth, mask, tint, inv_z),
        (_, Blend::Over) => over_scalar::<LAYER>(dst, depth, mask, tint, inv_z),
    }
}

/// The alpha byte of a blended pixel: opaque in the frame, and in the layer
/// whatever `layer` makes of the old one.
fn alpha_of<const LAYER: bool>(dst: u32, layer: impl Fn(u32) -> u32) -> u32 {
    if LAYER {
        layer(dst >> 24) << 24
    } else {
        0xff00_0000
    }
}

/// Additive blend of one pixel at coverage `m` (0..=255).
pub(super) fn add_px<const LAYER: bool>(dst: u32, m: u32, tint: u32) -> u32 {
    let ch = |shift: u32| {
        let s = (m * ((tint >> shift) & 255)) >> 8;
        (((dst >> shift) & 255) + s).min(255) << shift
    };
    alpha_of::<LAYER>(dst, |a| a) | ch(16) | ch(8) | ch(0)
}

/// Premultiplied "over" of one pixel at coverage `m` (0..=255).
pub(super) fn over_px<const LAYER: bool>(dst: u32, m: u32, tint: u32) -> u32 {
    let keep = 256 - ((m * (tint >> 24)) >> 8);
    let ch = |shift: u32| {
        let s = (m * ((tint >> shift) & 255)) >> 8;
        (s + ((((dst >> shift) & 255) * keep) >> 8)).min(255) << shift
    };
    alpha_of::<LAYER>(dst, |a| (a * keep) >> 8) | ch(16) | ch(8) | ch(0)
}

pub(super) fn add_scalar<const LAYER: bool>(
    dst: &mut [u32],
    depth: &[f32],
    mask: &[u8],
    tint: u32,
    inv_z: f32,
) {
    for ((d, &z), &m) in dst.iter_mut().zip(depth).zip(mask) {
        if inv_z >= z {
            *d = add_px::<LAYER>(*d, u32::from(m), tint);
        }
    }
}

pub(super) fn over_scalar<const LAYER: bool>(
    dst: &mut [u32],
    depth: &[f32],
    mask: &[u8],
    tint: u32,
    inv_z: f32,
) {
    for ((d, &z), &m) in dst.iter_mut().zip(depth).zip(mask) {
        if inv_z >= z {
            *d = over_px::<LAYER>(*d, u32::from(m), tint);
        }
    }
}

#[cfg(test)]
#[path = "kernels_tests.rs"]
mod kernels_tests;
