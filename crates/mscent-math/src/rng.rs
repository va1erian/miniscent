//! Tiny seeded xorshift64* generator, for reproducible property tests in this
//! and other crates. Not cryptographic; use it only for tests and procedural
//! content that must replay from a seed.

/// Deterministic pseudo-random generator: the same seed gives the same stream.
#[derive(Clone, Debug)]
pub struct Rng {
    state: u64,
}

impl Rng {
    /// Creates a generator. A zero seed is remapped because xorshift would
    /// otherwise stay at zero forever.
    pub fn new(seed: u64) -> Self {
        Self {
            state: if seed == 0 {
                0x9E37_79B9_7F4A_7C15
            } else {
                seed
            },
        }
    }

    /// Next 64 random bits.
    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.state;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.state = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// Uniform `f64` in `[0, 1)` using the top 53 bits.
    pub fn next_f64(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }

    /// Uniform `f64` in `[lo, hi)`.
    pub fn range_f64(&mut self, lo: f64, hi: f64) -> f64 {
        lo + (hi - lo) * self.next_f64()
    }

    /// Uniform `f32` in `[lo, hi)`.
    pub fn range_f32(&mut self, lo: f32, hi: f32) -> f32 {
        self.range_f64(f64::from(lo), f64::from(hi)) as f32
    }

    /// Uniform integer in `[0, n)`; returns 0 when `n` is 0.
    pub fn below(&mut self, n: u32) -> u32 {
        if n == 0 {
            return 0;
        }
        // Modulo bias is below 2^-32 for a 64-bit source; fine for tests.
        (self.next_u64() % u64::from(n)) as u32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_seed_same_stream() {
        let (mut a, mut b) = (Rng::new(7), Rng::new(7));
        for _ in 0..100 {
            assert_eq!(a.next_u64(), b.next_u64());
        }
    }

    #[test]
    fn zero_seed_is_usable() {
        let mut r = Rng::new(0);
        let (a, b) = (r.next_u64(), r.next_u64());
        assert_ne!(a, b);
    }

    #[test]
    fn ranges_hold() {
        let mut r = Rng::new(1);
        for _ in 0..10_000 {
            assert!((0.0..1.0).contains(&r.next_f64()));
            assert!(r.below(5) < 5);
            assert!((-2.0..3.0).contains(&r.range_f64(-2.0, 3.0)));
        }
        assert_eq!(r.below(0), 0);
    }
}
