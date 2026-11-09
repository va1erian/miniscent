//! Terrain layers: which voxel material each column of a range's surface takes,
//! by rules on the ground's height and slope (design in `docs/voxels.md`).
//!
//! A rule names a material and the band of height (as a share of the peak above
//! the range's ground) and of slope (rise over run) it covers. The first rule a
//! column falls in decides; a column no rule takes keeps the range's own material.
//! So the order is the priority: snow above a height, then cliff where steep, then
//! grass where flat and low, the range's material where nothing else is.
//!
//! A seeded noise moves each column's height before the rules read it, by up to
//! `jitter` of the peak, so a snow line or a tree line wanders instead of following
//! a contour. Like the range, the choice is pure: integer-hash noise and IEEE
//! arithmetic only, so the same range always paints the same columns.

use mscent_math::Vec3d;

use super::noise::noise3;
use crate::materials::{CLIFF, GRASS, SCREE, SNOW};

/// Most rules a range may have.
pub const MAX_LAYERS: usize = 8;
/// Largest jitter, as a share of the peak: past it the noise is the rule.
pub const MAX_JITTER: f64 = 1.0;
/// Largest bound a rule may give, either way: past every range's height and slope.
pub const MAX_BOUND: f64 = 1.0e6;
/// Jitter wavelengths per first-octave wavelength of the range.
const JITTER_FREQ: f64 = 6.0;
/// Seeds the jitter apart from the range's own noise.
const JITTER_SEED: u32 = 0x6c8e_9cf5;

/// One rule: the columns whose height and slope both lie in its bands, ends
/// included, take `material`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Layer {
    pub material: u8,
    /// Lowest and highest height, as shares of the peak above the ground.
    pub height: [f64; 2],
    /// Least and most slope, rise over run: 0 is flat, 1 is 45 degrees.
    pub slope: [f64; 2],
}

impl Layer {
    fn is_valid(&self) -> bool {
        let band_ok = |[lo, hi]: [f64; 2]| {
            lo.is_finite() && hi.is_finite() && lo <= hi && lo.abs() <= MAX_BOUND && hi <= MAX_BOUND
        };
        band_ok(self.height) && band_ok(self.slope) && self.slope[0] >= 0.0
    }

    fn holds(&self, height: f64, slope: f64) -> bool {
        let within = |[lo, hi]: [f64; 2], v: f64| lo <= v && v <= hi;
        within(self.height, height) && within(self.slope, slope)
    }
}

/// A range's rules, in priority order. Copy, so a range stays a plain value.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Layers {
    rules: [Layer; MAX_LAYERS],
    len: u8,
    /// How far the noise moves a column's height, as a share of the peak.
    pub jitter: f64,
}

/// Why a rule could not be added.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TooManyLayers;

impl Layers {
    /// No rules: the whole range is its own material.
    pub const NONE: Layers = Layers {
        rules: [Layer {
            material: 0,
            height: [0.0; 2],
            slope: [0.0; 2],
        }; MAX_LAYERS],
        len: 0,
        jitter: 0.0,
    };

    /// Snow on high ground that is not sheer, cliff on steep faces, grass on low
    /// flats and scree on the gentle slopes above them; the range's own material
    /// is what is left, the middling slopes between. On the Monument's range, of
    /// the columns above its ground, half are steeper than 0.39 and a tenth than
    /// 1.6, and half stand above 0.23 of the peak and a tenth above 0.65: these
    /// bands give each of the five a fair share.
    pub fn alpine() -> Layers {
        let mut layers = Layers {
            jitter: 0.06,
            ..Layers::NONE
        };
        let rules = [
            (SNOW, [0.45, MAX_BOUND], [0.0, 2.4]),
            (CLIFF, [-MAX_BOUND, MAX_BOUND], [1.0, MAX_BOUND]),
            (GRASS, [-MAX_BOUND, 0.18], [0.0, 0.45]),
            (SCREE, [-MAX_BOUND, 0.35], [0.0, 0.8]),
        ];
        for (material, height, slope) in rules {
            // The stock ids are all below 256, and four rules fit.
            let material = u8::try_from(material).unwrap_or(u8::MAX);
            let _ = layers.push(Layer {
                material,
                height,
                slope,
            });
        }
        layers
    }

    /// Adds a rule after the others, if there is room.
    pub fn push(&mut self, layer: Layer) -> Result<(), TooManyLayers> {
        let slot = self
            .rules
            .get_mut(usize::from(self.len))
            .ok_or(TooManyLayers)?;
        *slot = layer;
        self.len += 1;
        Ok(())
    }

    /// The rules, highest priority first.
    pub fn rules(&self) -> &[Layer] {
        self.rules.get(..usize::from(self.len)).unwrap_or(&[])
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Whether every bound is a finite number in range and the jitter is too.
    pub fn is_valid(&self) -> bool {
        let jitter_ok = self.jitter.is_finite() && (0.0..=MAX_JITTER).contains(&self.jitter);
        jitter_ok && self.rules().iter().all(Layer::is_valid)
    }

    /// The material of a column at `height` (share of the peak) and `slope`,
    /// whose plan position, in first-octave wavelengths, is `q`.
    pub(super) fn pick(&self, seed: u32, q: Vec3d, height: f64, slope: f64, base: u8) -> u8 {
        let moved = if self.jitter > 0.0 {
            let at = Vec3d::new(q.x * JITTER_FREQ, q.y * JITTER_FREQ, 0.71);
            height + self.jitter * noise3(seed ^ JITTER_SEED, at)
        } else {
            height
        };
        self.rules()
            .iter()
            .find(|l| l.holds(moved, slope))
            .map_or(base, |l| l.material)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(material: u8, height: [f64; 2], slope: [f64; 2]) -> Layer {
        Layer {
            material,
            height,
            slope,
        }
    }

    #[test]
    fn the_first_rule_that_holds_decides_and_none_leaves_the_base() {
        let mut l = Layers::NONE;
        l.push(rule(17, [0.6, 1.0], [0.0, 1.0])).unwrap();
        l.push(rule(16, [0.0, 1.0], [0.8, 9.0])).unwrap();
        let q = Vec3d::new(0.3, 0.4, 0.0);
        assert_eq!(l.pick(1, q, 0.7, 0.5, 4), 17);
        assert_eq!(l.pick(1, q, 0.7, 2.0, 4), 16);
        assert_eq!(l.pick(1, q, 0.2, 0.1, 4), 4);
        // Ends are inside.
        assert_eq!(l.pick(1, q, 0.6, 1.0, 4), 17);
    }

    #[test]
    fn jitter_moves_the_line_by_at_most_its_share_and_the_same_way_each_time() {
        let mut l = Layers {
            jitter: 0.1,
            ..Layers::NONE
        };
        l.push(rule(17, [0.5, 1.0], [0.0, 9.0])).unwrap();
        let mut moved = 0;
        for i in 0..400 {
            let q = Vec3d::new(f64::from(i) * 0.037, f64::from(i) * 0.011, 0.0);
            // Well clear of the line, the noise never reaches it.
            assert_eq!(l.pick(3, q, 0.75, 0.0, 4), 17);
            assert_eq!(l.pick(3, q, 0.25, 0.0, 4), 4);
            let near = l.pick(3, q, 0.5, 0.0, 4);
            assert_eq!(near, l.pick(3, q, 0.5, 0.0, 4));
            moved += usize::from(near == 4);
        }
        assert!(moved > 50 && moved < 350, "{moved} of 400 below the line");
    }

    #[test]
    fn bad_bounds_and_too_many_rules_are_refused() {
        let ok = rule(1, [0.0, 1.0], [0.0, 1.0]);
        let bad = [
            rule(1, [f64::NAN, 1.0], [0.0, 1.0]),
            rule(1, [0.0, f64::INFINITY], [0.0, 1.0]),
            rule(1, [0.8, 0.2], [0.0, 1.0]),
            rule(1, [0.0, 1.0], [-0.5, 1.0]),
            rule(1, [-2.0e6, 1.0], [0.0, 1.0]),
            rule(1, [0.0, 1.0], [0.0, 2.0e6]),
        ];
        for b in bad {
            let mut l = Layers::NONE;
            l.push(b).unwrap();
            assert!(!l.is_valid(), "{b:?}");
        }
        for jitter in [f64::NAN, -0.1, 1.5] {
            assert!(
                !Layers {
                    jitter,
                    ..Layers::NONE
                }
                .is_valid(),
                "{jitter}"
            );
        }
        let mut l = Layers::NONE;
        for _ in 0..MAX_LAYERS {
            l.push(ok).unwrap();
        }
        assert_eq!(l.push(ok), Err(TooManyLayers));
        assert_eq!(l.rules().len(), MAX_LAYERS);
        assert!(l.is_valid() && Layers::alpine().is_valid());
    }
}
