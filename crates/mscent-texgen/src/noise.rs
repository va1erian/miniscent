//! Tiling gradient noise. Each octave is a lattice whose cell count is a whole
//! number per axis, wrapped at the tile, so the result repeats by construction.
//! A lattice with no cells on an axis does not vary along it, which is how
//! brushed metal gets its streaks.

/// Lattice directions; a table because the platform's `sin` is not deterministic.
const R: f32 = core::f32::consts::FRAC_1_SQRT_2;
const GRAD: [(f32, f32); 8] = [
    (1.0, 0.0),
    (-1.0, 0.0),
    (0.0, 1.0),
    (0.0, -1.0),
    (R, R),
    (-R, R),
    (R, -R),
    (-R, -R),
];

/// How the octaves add up.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fold {
    /// Signed octaves added.
    Fbm,
    /// Each octave folded at zero into ridges.
    Ridged,
}

/// A tiling noise pattern.
#[derive(Clone, Copy, Debug)]
pub struct Noise {
    /// Lattice cells per tile of the first octave, per axis.
    pub freq: (u32, u32),
    pub octaves: u32,
    /// Amplitude of each octave against the one before.
    pub persistence: f32,
    pub seed: u32,
    pub fold: Fold,
}

/// A lattice point's direction: a stateless hash, so evaluating one texel needs
/// no table.
#[inline]
fn gradient(ix: u32, iy: u32, seed: u32) -> (f32, f32) {
    let mut h = ix
        .wrapping_mul(0x9E37_79B1)
        .wrapping_add(iy.wrapping_mul(0x85EB_CA77))
        .wrapping_add(seed.wrapping_mul(0xC2B2_AE3D));
    h ^= h >> 15;
    h = h.wrapping_mul(0x2767_79B1);
    h ^= h >> 13;
    h = h.wrapping_mul(0x1656_67B1);
    GRAD[(h >> 29) as usize]
}

#[inline]
fn fade(t: f32) -> f32 {
    t * t * t * (t * (t * 6.0 - 15.0) + 10.0)
}

/// Wraps `u` into a lattice of `cells`: the two lattice lines around it, and how
/// far along it is. No cells: one line, nowhere along.
#[inline]
fn axis(u: f32, cells: u32) -> (u32, u32, f32) {
    if cells == 0 {
        return (0, 0, 0.0);
    }
    let x = u * cells as f32;
    let cell = x.floor();
    let t = x - cell;
    let i = (cell as i64).rem_euclid(i64::from(cells)) as u32;
    (i, (i + 1) % cells, t)
}

/// One octave, about `-1.0..=1.0`.
fn octave(u: f32, v: f32, freq: (u32, u32), seed: u32) -> f32 {
    let (x0, x1, tx) = axis(u, freq.0);
    let (y0, y1, ty) = axis(v, freq.1);
    let dot = |g: (f32, f32), dx: f32, dy: f32| g.0 * dx + g.1 * dy;
    let a = dot(gradient(x0, y0, seed), tx, ty);
    let b = dot(gradient(x1, y0, seed), tx - 1.0, ty);
    let c = dot(gradient(x0, y1, seed), tx, ty - 1.0);
    let d = dot(gradient(x1, y1, seed), tx - 1.0, ty - 1.0);
    let (fx, fy) = (fade(tx), fade(ty));
    let top = a + (b - a) * fx;
    let bottom = c + (d - c) * fx;
    (top + (bottom - top) * fy) * core::f32::consts::SQRT_2
}

impl Noise {
    /// The raw value at normalized coordinates; the `noise` operator scales a
    /// whole tile to the full range.
    pub fn at(&self, u: f32, v: f32) -> f32 {
        let (mut sum, mut amp) = (0.0, 1.0);
        let (mut fx, mut fy) = self.freq;
        for o in 0..self.octaves {
            let n = octave(u, v, (fx, fy), self.seed.wrapping_add(o));
            sum += amp
                * match self.fold {
                    Fold::Fbm => n,
                    Fold::Ridged => 1.0 - 2.0 * n.abs(),
                };
            amp *= self.persistence;
            fx = fx.saturating_mul(2);
            fy = fy.saturating_mul(2);
        }
        sum
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn noise(freq: (u32, u32)) -> Noise {
        Noise {
            freq,
            octaves: 4,
            persistence: 0.5,
            seed: 11,
            fold: Fold::Fbm,
        }
    }

    #[test]
    fn it_repeats_at_the_tile_for_every_frequency() {
        for f in 1..=32 {
            let n = noise((f, 33 - f));
            for i in 0..64 {
                let (u, v) = (i as f32 / 64.0, (64 - i) as f32 / 64.0);
                assert!((n.at(u, v) - n.at(u + 1.0, v)).abs() < 1e-3, "u, f={f}");
                assert!((n.at(u, v) - n.at(u, v - 1.0)).abs() < 1e-3, "v, f={f}");
            }
        }
    }

    #[test]
    fn no_cells_on_an_axis_means_no_change_along_it() {
        let n = noise((0, 8));
        for i in 0..50 {
            assert_eq!(n.at(i as f32 / 50.0, 0.37), n.at(0.0, 0.37));
        }
        assert_ne!(n.at(0.0, 0.1), n.at(0.0, 0.37));
    }

    #[test]
    fn seeds_change_it_and_the_same_seed_does_not() {
        let (a, mut b) = (noise((4, 4)), noise((4, 4)));
        assert_eq!(a.at(0.3, 0.3), b.at(0.3, 0.3));
        b.seed = 12;
        assert_ne!(a.at(0.3, 0.3), b.at(0.3, 0.3));
    }

    #[test]
    fn huge_frequencies_and_garbage_coordinates_do_not_panic() {
        let n = Noise {
            freq: (u32::MAX, u32::MAX),
            octaves: 8,
            persistence: 1.0,
            seed: u32::MAX,
            fold: Fold::Ridged,
        };
        for u in [0.5, f32::NAN, f32::INFINITY, -1e30] {
            let _ = n.at(u, u);
        }
    }
}
