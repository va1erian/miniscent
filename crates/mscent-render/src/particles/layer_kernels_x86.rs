//! x86-64 compositing kernels behind [`super::composite_row`]. Each takes whole
//! vectors of frame pixels and leaves the last few to the scalar code, which is
//! the definition these must match.
//!
//! Each layer pixel is repeated for its two frame pixels, then everything is
//! widened to 16-bit lanes and packed back as in `kernels_x86.rs`. The keep
//! factor is the layer pixel's alpha lane spread over its colour lanes.

use std::arch::x86_64::*;

use super::{CLEAR, composite_scalar, nearest_scalar};

const ALPHA: i32 = 0xff00_0000u32 as i32;

/// Caller guarantees nothing beyond the baseline.
#[target_feature(enable = "sse2")]
pub(super) fn composite_sse2(dst: &mut [u32], layer: &[u32]) {
    let (zero, one) = (_mm_setzero_si128(), _mm_set1_epi16(1));
    let alpha = _mm_set1_epi32(ALPHA);
    let (d, _) = dst.as_chunks_mut::<4>();
    let (l, _) = layer.as_chunks::<2>();
    let done = d.len().min(l.len()) * 4;
    // `p` holds the frame's channels shifted up a byte, so `mulhi` by `keep`
    // is `channel * keep >> 8` exactly.
    let mix = |layer: __m128i, p: __m128i| {
        let keep = _mm_shufflehi_epi16::<0xff>(_mm_shufflelo_epi16::<0xff>(layer));
        _mm_add_epi16(layer, _mm_mulhi_epu16(p, _mm_add_epi16(keep, one)))
    };
    for (d, l) in d.iter_mut().zip(l) {
        if *l == [CLEAR; 2] {
            continue;
        }
        // SAFETY: `l` is exactly two u32 = 8 bytes; unaligned load.
        let pair = unsafe { _mm_loadl_epi64(l.as_ptr().cast()) };
        // Each layer pixel twice: l0 l0 l1 l1.
        let l4 = _mm_unpacklo_epi32(pair, pair);
        // SAFETY: `d` is exactly four u32 = 16 bytes; unaligned load.
        let p = unsafe { _mm_loadu_si128(d.as_ptr().cast()) };
        let lo = mix(_mm_unpacklo_epi8(l4, zero), _mm_unpacklo_epi8(zero, p));
        let hi = mix(_mm_unpackhi_epi8(l4, zero), _mm_unpackhi_epi8(zero, p));
        // The frame's own alpha byte stays.
        let out = _mm_or_si128(
            _mm_andnot_si128(alpha, _mm_packus_epi16(lo, hi)),
            _mm_and_si128(alpha, p),
        );
        // SAFETY: same 16-byte chunk as the load.
        unsafe { _mm_storeu_si128(d.as_mut_ptr().cast(), out) };
    }
    composite_scalar(&mut dst[done..], &layer[done / 2..]);
}

/// Caller guarantees that the CPU supports AVX2.
#[target_feature(enable = "avx2")]
pub(super) fn composite_avx2(dst: &mut [u32], layer: &[u32]) {
    let (zero, one) = (_mm256_setzero_si256(), _mm256_set1_epi16(1));
    let alpha = _mm256_set1_epi32(ALPHA);
    // Each layer pixel twice: l0 l0 l1 l1 l2 l2 l3 l3.
    let twice = _mm256_setr_epi32(0, 0, 1, 1, 2, 2, 3, 3);
    // Shuffles that send the alpha byte of a doubled pixel to every 16-bit lane
    // of its pixel, for the low and high unpack of a 128-bit lane (a negative
    // index zeroes the byte).
    let (lo_keep, hi_keep) = (
        _mm256_setr_epi8(
            3, -1, 3, -1, 3, -1, 3, -1, 7, -1, 7, -1, 7, -1, 7, -1, 3, -1, 3, -1, 3, -1, 3, -1, 7,
            -1, 7, -1, 7, -1, 7, -1,
        ),
        _mm256_setr_epi8(
            11, -1, 11, -1, 11, -1, 11, -1, 15, -1, 15, -1, 15, -1, 15, -1, 11, -1, 11, -1, 11, -1,
            11, -1, 15, -1, 15, -1, 15, -1, 15, -1,
        ),
    );
    let (d, _) = dst.as_chunks_mut::<8>();
    let (l, _) = layer.as_chunks::<4>();
    let done = d.len().min(l.len()) * 8;
    for (d, l) in d.iter_mut().zip(l) {
        if *l == [CLEAR; 4] {
            continue;
        }
        // SAFETY: `l` is exactly four u32 = 16 bytes; unaligned load.
        let four = unsafe { _mm_loadu_si128(l.as_ptr().cast()) };
        // The upper half of the cast is undefined and never selected.
        let l8 = _mm256_permutevar8x32_epi32(_mm256_castsi128_si256(four), twice);
        // SAFETY: `d` is exactly eight u32 = 32 bytes; unaligned load.
        let p = unsafe { _mm256_loadu_si256(d.as_ptr().cast()) };
        // As in `composite_sse2`; the unpacks work inside 128-bit lanes exactly
        // as the layer's do.
        let mix = |colour: __m256i, p: __m256i, keep: __m256i| {
            let keep = _mm256_add_epi16(keep, one);
            _mm256_add_epi16(colour, _mm256_mulhi_epu16(p, keep))
        };
        let lo = mix(
            _mm256_unpacklo_epi8(l8, zero),
            _mm256_unpacklo_epi8(zero, p),
            _mm256_shuffle_epi8(l8, lo_keep),
        );
        let hi = mix(
            _mm256_unpackhi_epi8(l8, zero),
            _mm256_unpackhi_epi8(zero, p),
            _mm256_shuffle_epi8(l8, hi_keep),
        );
        let out = _mm256_or_si256(
            _mm256_andnot_si256(alpha, _mm256_packus_epi16(lo, hi)),
            _mm256_and_si256(alpha, p),
        );
        // SAFETY: same 32-byte chunk as the load.
        unsafe { _mm256_storeu_si256(d.as_mut_ptr().cast(), out) };
    }
    composite_sse2(&mut dst[done..], &layer[done / 2..]);
}

/// Caller guarantees nothing beyond the baseline.
#[target_feature(enable = "sse2")]
pub(super) fn nearest_sse2(out: &mut [f32], top: &[f32], bottom: &[f32]) {
    let (o, _) = out.as_chunks_mut::<4>();
    // Four blocks come from eight pixels of each row, two vectors apiece.
    let (t, _) = top.as_chunks::<4>();
    let (t, _) = t.as_chunks::<2>();
    let (b, _) = bottom.as_chunks::<4>();
    let (b, _) = b.as_chunks::<2>();
    let mut done = 0;
    for ((o, t), b) in o.iter_mut().zip(t).zip(b) {
        // SAFETY: each of these is a `[f32; 4]` = 16 bytes; unaligned loads.
        let (t0, t1, b0, b1) = unsafe {
            (
                _mm_loadu_ps(t[0].as_ptr()),
                _mm_loadu_ps(t[1].as_ptr()),
                _mm_loadu_ps(b[0].as_ptr()),
                _mm_loadu_ps(b[1].as_ptr()),
            )
        };
        let (m0, m1) = (_mm_max_ps(t0, b0), _mm_max_ps(t1, b1));
        // Even and odd pixels of the eight, one block per lane.
        let even = _mm_shuffle_ps::<0b10_00_10_00>(m0, m1);
        let odd = _mm_shuffle_ps::<0b11_01_11_01>(m0, m1);
        // SAFETY: `o` is four f32 = 16 bytes; unaligned store.
        unsafe { _mm_storeu_ps(o.as_mut_ptr(), _mm_max_ps(even, odd)) };
        done += 4;
    }
    nearest_scalar(&mut out[done..], &top[2 * done..], &bottom[2 * done..]);
}
