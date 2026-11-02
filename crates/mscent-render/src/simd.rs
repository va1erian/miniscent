//! Whole-buffer colour kernels with an SSE2 path, an AVX2 path and a scalar
//! fallback.
//!
//! Every path is bit-identical to the scalar definition ([`mul_rgb`] and
//! [`lerp_rgb`]); the tests assert that, so the choice of path can never show
//! up in a screenshot. The vector paths work on 16-bit lanes because the
//! products fit: `channel * k` with `k <= 257` and `channel * (256 - t) +
//! target * t` with `t <= 256` both stay at or below 65535.
//!
//! The AVX2 path is chosen at run time; SSE2 is part of the x86-64 baseline
//! and other architectures use the scalar code.

// The crate denies unsafe code; this module is an exception. Each unsafe
// block calls a `target_feature` kernel only after checking the CPU supports
// it, or does a bounds-exact vector load or store, and says so in a SAFETY
// comment.
#![allow(unsafe_code)]

use crate::shade::{lerp_rgb, mul_rgb};

#[cfg(target_arch = "x86_64")]
#[path = "simd_x86.rs"]
mod x86;

/// Which implementation runs a kernel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    Scalar,
    Sse2,
    Avx2,
}

impl Level {
    /// Every level, slowest first.
    pub const ALL: [Level; 3] = [Level::Scalar, Level::Sse2, Level::Avx2];

    /// Whether this CPU can run the level.
    pub fn is_supported(self) -> bool {
        match self {
            Level::Scalar => true,
            Level::Sse2 => cfg!(target_arch = "x86_64"),
            Level::Avx2 => avx2_available(),
        }
    }

    /// The fastest level this CPU supports.
    pub fn best() -> Level {
        if Level::Avx2.is_supported() {
            Level::Avx2
        } else if Level::Sse2.is_supported() {
            Level::Sse2
        } else {
            Level::Scalar
        }
    }
}

#[cfg(target_arch = "x86_64")]
fn avx2_available() -> bool {
    is_x86_feature_detected!("avx2")
}

#[cfg(not(target_arch = "x86_64"))]
fn avx2_available() -> bool {
    false
}

/// Scale the colour channels of every pixel by `k / 256` (see [`mul_rgb`]).
pub fn scale_rgb(pixels: &mut [u32], k: u32) {
    scale_rgb_with(Level::best(), pixels, k);
}

/// Blend every pixel toward `color` by `t / 256` (see [`lerp_rgb`]).
pub fn lerp_toward(pixels: &mut [u32], color: u32, t: u32) {
    lerp_toward_with(Level::best(), pixels, color, t);
}

/// [`scale_rgb`] on a chosen level; an unsupported level runs the scalar code.
pub fn scale_rgb_with(level: Level, pixels: &mut [u32], k: u32) {
    // Larger factors overflow the 16-bit lanes, so they take the exact path.
    let level = if k > 257 { Level::Scalar } else { level };
    match usable(level) {
        #[cfg(target_arch = "x86_64")]
        // SAFETY: `usable` only returns Avx2 after run-time detection, and
        // `k <= 257` was checked above.
        Level::Avx2 => unsafe { x86::scale_avx2(pixels, k) },
        #[cfg(target_arch = "x86_64")]
        // SAFETY: SSE2 is part of the x86-64 baseline; `k <= 257` as above.
        Level::Sse2 => unsafe { x86::scale_sse2(pixels, k) },
        _ => scale_scalar(pixels, k),
    }
}

/// [`lerp_toward`] on a chosen level; an unsupported level runs the scalar
/// code.
pub fn lerp_toward_with(level: Level, pixels: &mut [u32], color: u32, t: u32) {
    let t = t.min(256);
    match usable(level) {
        #[cfg(target_arch = "x86_64")]
        // SAFETY: `usable` only returns Avx2 after run-time detection, and
        // `t <= 256` was clamped above.
        Level::Avx2 => unsafe { x86::lerp_avx2(pixels, color, t) },
        #[cfg(target_arch = "x86_64")]
        // SAFETY: SSE2 is part of the x86-64 baseline; `t <= 256` as above.
        Level::Sse2 => unsafe { x86::lerp_sse2(pixels, color, t) },
        _ => lerp_scalar(pixels, color, t),
    }
}

/// `level` if this CPU can run it, else the scalar level.
pub(crate) fn usable(level: Level) -> Level {
    if level.is_supported() {
        level
    } else {
        Level::Scalar
    }
}

fn scale_scalar(pixels: &mut [u32], k: u32) {
    for p in pixels {
        *p = mul_rgb(*p, k);
    }
}

fn lerp_scalar(pixels: &mut [u32], color: u32, t: u32) {
    for p in pixels {
        *p = lerp_rgb(*p, color, t);
    }
}

#[cfg(test)]
#[path = "simd_tests.rs"]
mod simd_tests;
