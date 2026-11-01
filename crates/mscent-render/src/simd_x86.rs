//! x86-64 kernels behind [`super`]. Each processes whole vectors and leaves
//! the last `len % lanes` pixels to the scalar code, which is the definition
//! the vector code must match.

use std::arch::x86_64::*;

use super::{lerp_scalar, scale_scalar};

const OPAQUE: i32 = 0xff00_0000u32 as i32;

/// Caller guarantees `k <= 257`.
#[target_feature(enable = "sse2")]
pub(super) fn scale_sse2(pixels: &mut [u32], k: u32) {
    let kv = _mm_set1_epi16(k as i16);
    let zero = _mm_setzero_si128();
    let opaque = _mm_set1_epi32(OPAQUE);
    let (chunks, rest) = pixels.as_chunks_mut::<4>();
    for c in chunks {
        // SAFETY: `c` is exactly four u32 = 16 bytes; unaligned load.
        let p = unsafe { _mm_loadu_si128(c.as_ptr().cast()) };
        let lo = _mm_srli_epi16::<8>(_mm_mullo_epi16(_mm_unpacklo_epi8(p, zero), kv));
        let hi = _mm_srli_epi16::<8>(_mm_mullo_epi16(_mm_unpackhi_epi8(p, zero), kv));
        let r = _mm_or_si128(_mm_packus_epi16(lo, hi), opaque);
        // SAFETY: same 16-byte chunk as the load.
        unsafe { _mm_storeu_si128(c.as_mut_ptr().cast(), r) };
    }
    scale_scalar(rest, k);
}

/// Caller guarantees `t <= 256`.
#[target_feature(enable = "sse2")]
pub(super) fn lerp_sse2(pixels: &mut [u32], color: u32, t: u32) {
    let zero = _mm_setzero_si128();
    let keep = _mm_set1_epi16((256 - t) as i16);
    // color * t per channel, the same for every pixel.
    let target = _mm_mullo_epi16(
        _mm_unpacklo_epi8(_mm_set1_epi32(color as i32), zero),
        _mm_set1_epi16(t as i16),
    );
    let opaque = _mm_set1_epi32(OPAQUE);
    let (chunks, rest) = pixels.as_chunks_mut::<4>();
    for c in chunks {
        // SAFETY: `c` is exactly four u32 = 16 bytes; unaligned load.
        let p = unsafe { _mm_loadu_si128(c.as_ptr().cast()) };
        let mix = |x| _mm_srli_epi16::<8>(_mm_add_epi16(_mm_mullo_epi16(x, keep), target));
        let lo = mix(_mm_unpacklo_epi8(p, zero));
        let hi = mix(_mm_unpackhi_epi8(p, zero));
        let r = _mm_or_si128(_mm_packus_epi16(lo, hi), opaque);
        // SAFETY: same 16-byte chunk as the load.
        unsafe { _mm_storeu_si128(c.as_mut_ptr().cast(), r) };
    }
    lerp_scalar(rest, color, t);
}

/// Caller guarantees `k <= 257` and that the CPU supports AVX2.
#[target_feature(enable = "avx2")]
pub(super) fn scale_avx2(pixels: &mut [u32], k: u32) {
    let kv = _mm256_set1_epi16(k as i16);
    let zero = _mm256_setzero_si256();
    let opaque = _mm256_set1_epi32(OPAQUE);
    let (chunks, rest) = pixels.as_chunks_mut::<8>();
    for c in chunks {
        // SAFETY: `c` is exactly eight u32 = 32 bytes; unaligned load.
        let p = unsafe { _mm256_loadu_si256(c.as_ptr().cast()) };
        // Unpack and pack both work within 128-bit lanes, so pixel order
        // survives the round trip.
        let lo = _mm256_srli_epi16::<8>(_mm256_mullo_epi16(_mm256_unpacklo_epi8(p, zero), kv));
        let hi = _mm256_srli_epi16::<8>(_mm256_mullo_epi16(_mm256_unpackhi_epi8(p, zero), kv));
        let r = _mm256_or_si256(_mm256_packus_epi16(lo, hi), opaque);
        // SAFETY: same 32-byte chunk as the load.
        unsafe { _mm256_storeu_si256(c.as_mut_ptr().cast(), r) };
    }
    scale_scalar(rest, k);
}

/// Caller guarantees `t <= 256` and that the CPU supports AVX2.
#[target_feature(enable = "avx2")]
pub(super) fn lerp_avx2(pixels: &mut [u32], color: u32, t: u32) {
    let zero = _mm256_setzero_si256();
    let keep = _mm256_set1_epi16((256 - t) as i16);
    // color * t per channel; unpacklo of a broadcast pixel repeats it in
    // both 128-bit lanes, matching how the data lanes are unpacked.
    let target = _mm256_mullo_epi16(
        _mm256_unpacklo_epi8(_mm256_set1_epi32(color as i32), zero),
        _mm256_set1_epi16(t as i16),
    );
    let opaque = _mm256_set1_epi32(OPAQUE);
    let (chunks, rest) = pixels.as_chunks_mut::<8>();
    for c in chunks {
        // SAFETY: `c` is exactly eight u32 = 32 bytes; unaligned load.
        let p = unsafe { _mm256_loadu_si256(c.as_ptr().cast()) };
        let mix = |x| _mm256_srli_epi16::<8>(_mm256_add_epi16(_mm256_mullo_epi16(x, keep), target));
        let lo = mix(_mm256_unpacklo_epi8(p, zero));
        let hi = mix(_mm256_unpackhi_epi8(p, zero));
        let r = _mm256_or_si256(_mm256_packus_epi16(lo, hi), opaque);
        // SAFETY: same 32-byte chunk as the load.
        unsafe { _mm256_storeu_si256(c.as_mut_ptr().cast(), r) };
    }
    lerp_scalar(rest, color, t);
}
