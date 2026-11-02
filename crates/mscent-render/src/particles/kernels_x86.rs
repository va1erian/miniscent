//! x86-64 sprite-row kernels behind [`super::blend_row`]. Each takes whole
//! vectors of pixels and leaves the last few to the scalar code, which is the
//! definition these must match.
//!
//! Pixels are widened to 16-bit lanes (two per half vector, four lanes each)
//! and packed back in order, as in `simd_x86.rs`. Both blends are "widen, mix,
//! pack with saturation", so one runner per width takes the mix as a closure.
//! Pixels that fail the depth test are computed too and then replaced by their
//! old value, which is cheaper than branching per pixel; a whole vector that
//! fails skips the work.

use std::arch::x86_64::*;

use super::{add_scalar, over_scalar};

const OPAQUE: i32 = 0xff00_0000u32 as i32;

/// Caller guarantees the slices have equal length.
#[target_feature(enable = "sse2")]
pub(super) fn add_sse2(dst: &mut [u32], depth: &[f32], mask: &[u8], tint: u32, inv_z: f32) {
    let tint_w = _mm_unpacklo_epi8(_mm_set1_epi32(tint as i32), _mm_setzero_si128());
    let done = run_sse2(dst, depth, mask, inv_z, |m, p| {
        _mm_add_epi16(p, _mm_srli_epi16::<8>(_mm_mullo_epi16(m, tint_w)))
    });
    add_scalar(&mut dst[done..], &depth[done..], &mask[done..], tint, inv_z);
}

/// Caller guarantees the slices have equal length.
#[target_feature(enable = "sse2")]
pub(super) fn over_sse2(dst: &mut [u32], depth: &[f32], mask: &[u8], tint: u32, inv_z: f32) {
    let tint_w = _mm_unpacklo_epi8(_mm_set1_epi32(tint as i32), _mm_setzero_si128());
    let alpha = _mm_set1_epi16((tint >> 24) as i16);
    let full = _mm_set1_epi16(256);
    let done = run_sse2(dst, depth, mask, inv_z, |m, p| {
        let s = _mm_srli_epi16::<8>(_mm_mullo_epi16(m, tint_w));
        let keep = _mm_sub_epi16(full, _mm_srli_epi16::<8>(_mm_mullo_epi16(m, alpha)));
        _mm_add_epi16(s, _mm_srli_epi16::<8>(_mm_mullo_epi16(p, keep)))
    });
    over_scalar(&mut dst[done..], &depth[done..], &mask[done..], tint, inv_z);
}

/// Runs `mix(coverage, pixel)` on 16-bit lanes over every whole group of four
/// pixels and returns how many pixels that covered.
#[target_feature(enable = "sse2")]
fn run_sse2(
    dst: &mut [u32],
    depth: &[f32],
    mask: &[u8],
    inv_z: f32,
    mix: impl Fn(__m128i, __m128i) -> __m128i,
) -> usize {
    let zero = _mm_setzero_si128();
    let opaque = _mm_set1_epi32(OPAQUE);
    let z = _mm_set1_ps(inv_z);
    let (dst, _) = dst.as_chunks_mut::<4>();
    let (depth, _) = depth.as_chunks::<4>();
    let (mask, _) = mask.as_chunks::<4>();
    let done = dst.len().min(depth.len()).min(mask.len()) * 4;
    for ((d, s), m) in dst.iter_mut().zip(depth).zip(mask) {
        // SAFETY: `s` is exactly four f32 = 16 bytes; unaligned load.
        let stored = unsafe { _mm_loadu_ps(s.as_ptr()) };
        let pass = _mm_cmpge_ps(z, stored);
        if _mm_movemask_ps(pass) == 0 {
            continue;
        }
        let pass = _mm_castps_si128(pass);
        // Each coverage byte repeated across the four lanes of its pixel.
        let m16 = _mm_unpacklo_epi8(_mm_cvtsi32_si128(i32::from_le_bytes(*m)), zero);
        let m16 = _mm_unpacklo_epi16(m16, m16);
        let (m_lo, m_hi) = (_mm_unpacklo_epi32(m16, m16), _mm_unpackhi_epi32(m16, m16));
        // SAFETY: `d` is exactly four u32 = 16 bytes; unaligned load.
        let p = unsafe { _mm_loadu_si128(d.as_ptr().cast()) };
        let lo = mix(m_lo, _mm_unpacklo_epi8(p, zero));
        let hi = mix(m_hi, _mm_unpackhi_epi8(p, zero));
        let new = _mm_or_si128(_mm_packus_epi16(lo, hi), opaque);
        let out = _mm_or_si128(_mm_and_si128(pass, new), _mm_andnot_si128(pass, p));
        // SAFETY: same 16-byte chunk as the load.
        unsafe { _mm_storeu_si128(d.as_mut_ptr().cast(), out) };
    }
    done
}

/// Caller guarantees equal lengths and that the CPU supports AVX2.
#[target_feature(enable = "avx2")]
pub(super) fn add_avx2(dst: &mut [u32], depth: &[f32], mask: &[u8], tint: u32, inv_z: f32) {
    // Unpacking a broadcast pixel repeats it in both 128-bit lanes, matching
    // how the data lanes are unpacked.
    let tint_w = _mm256_unpacklo_epi8(_mm256_set1_epi32(tint as i32), _mm256_setzero_si256());
    let done = run_avx2(dst, depth, mask, inv_z, |m, p| {
        _mm256_add_epi16(p, _mm256_srli_epi16::<8>(_mm256_mullo_epi16(m, tint_w)))
    });
    // A sprite row is often narrower than eight pixels or ends in a few more.
    add_sse2(&mut dst[done..], &depth[done..], &mask[done..], tint, inv_z);
}

/// Caller guarantees equal lengths and that the CPU supports AVX2.
#[target_feature(enable = "avx2")]
pub(super) fn over_avx2(dst: &mut [u32], depth: &[f32], mask: &[u8], tint: u32, inv_z: f32) {
    let tint_w = _mm256_unpacklo_epi8(_mm256_set1_epi32(tint as i32), _mm256_setzero_si256());
    let alpha = _mm256_set1_epi16((tint >> 24) as i16);
    let full = _mm256_set1_epi16(256);
    let done = run_avx2(dst, depth, mask, inv_z, |m, p| {
        let s = _mm256_srli_epi16::<8>(_mm256_mullo_epi16(m, tint_w));
        let keep = _mm256_sub_epi16(full, _mm256_srli_epi16::<8>(_mm256_mullo_epi16(m, alpha)));
        _mm256_add_epi16(s, _mm256_srli_epi16::<8>(_mm256_mullo_epi16(p, keep)))
    });
    over_sse2(&mut dst[done..], &depth[done..], &mask[done..], tint, inv_z);
}

/// [`run_sse2`] for groups of eight pixels.
#[target_feature(enable = "avx2")]
fn run_avx2(
    dst: &mut [u32],
    depth: &[f32],
    mask: &[u8],
    inv_z: f32,
    mix: impl Fn(__m256i, __m256i) -> __m256i,
) -> usize {
    let zero = _mm256_setzero_si256();
    let opaque = _mm256_set1_epi32(OPAQUE);
    let z = _mm256_set1_ps(inv_z);
    let (dst, _) = dst.as_chunks_mut::<8>();
    let (depth, _) = depth.as_chunks::<8>();
    let (mask, _) = mask.as_chunks::<8>();
    let done = dst.len().min(depth.len()).min(mask.len()) * 8;
    for ((d, s), m) in dst.iter_mut().zip(depth).zip(mask) {
        // SAFETY: `s` is exactly eight f32 = 32 bytes; unaligned load.
        let stored = unsafe { _mm256_loadu_ps(s.as_ptr()) };
        let pass = _mm256_cmp_ps::<_CMP_GE_OQ>(z, stored);
        if _mm256_movemask_ps(pass) == 0 {
            continue;
        }
        let pass = _mm256_castps_si256(pass);
        // One coverage byte per 32-bit lane, repeated into both halves of the
        // lane, then spread so each pixel's four 16-bit lanes hold it. The
        // unpacks work inside 128-bit lanes exactly as the data's do.
        let wide = _mm256_cvtepu8_epi32(_mm_cvtsi64_si128(i64::from_le_bytes(*m)));
        let wide = _mm256_or_si256(wide, _mm256_slli_epi32::<16>(wide));
        let (m_lo, m_hi) = (
            _mm256_unpacklo_epi32(wide, wide),
            _mm256_unpackhi_epi32(wide, wide),
        );
        // SAFETY: `d` is exactly eight u32 = 32 bytes; unaligned load.
        let p = unsafe { _mm256_loadu_si256(d.as_ptr().cast()) };
        let lo = mix(m_lo, _mm256_unpacklo_epi8(p, zero));
        let hi = mix(m_hi, _mm256_unpackhi_epi8(p, zero));
        let new = _mm256_or_si256(_mm256_packus_epi16(lo, hi), opaque);
        let out = _mm256_or_si256(_mm256_and_si256(pass, new), _mm256_andnot_si256(pass, p));
        // SAFETY: same 32-byte chunk as the load.
        unsafe { _mm256_storeu_si256(d.as_mut_ptr().cast(), out) };
    }
    done
}
