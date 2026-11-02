//! Stalactites on a ceiling, stalagmites on a floor, or spikes on a wall:
//! tapered drips that grow out of the surface along its normal.

use std::f64::consts::TAU;

use mscent_math::rng::Rng;

use super::Plan;
use super::site::{Frame, Site};

/// Drips per site.
const MIN_DRIPS: u32 = 7;
const EXTRA_DRIPS: u32 = 6;

pub(super) fn grow(plan: &mut Plan, f: &Frame, site: &Site, rng: &mut Rng) {
    let reach = 3.0 * site.size;
    let drips = MIN_DRIPS + rng.below(EXTRA_DRIPS);
    for _ in 0..drips {
        // The square root spreads drips evenly over the disc, not toward its middle.
        let (r, a) = (reach * rng.next_f64().sqrt(), rng.range_f64(0.0, TAU));
        let (x, y) = (r * a.cos(), r * a.sin());
        let edge = r / reach;
        let base = site.size * rng.range_f64(0.22, 0.45);
        // Drips thin out and shorten toward the edge, like a real drip field.
        let len = site.size * rng.range_f64(1.0, 3.2) * (1.0 - 0.55 * edge);
        drip(plan, f, (x, y), (base, len), rng, site.material);
    }
}

/// One drip: a thick root sunk into the surface, a waist, and a thin tip that
/// wanders a little off the line, with a flare where it meets the rock.
fn drip(plan: &mut Plan, f: &Frame, at: (f64, f64), (base, len): (f64, f64), rng: &mut Rng, m: u8) {
    let wander = base * 0.5;
    let sway = |rng: &mut Rng| rng.range_f64(-wander, wander);
    let root = f.at(at.0, at.1, -base);
    let waist = f.at(at.0 + sway(rng), at.1 + sway(rng), len * 0.55);
    let tip = f.at(at.0 + 2.0 * sway(rng), at.1 + 2.0 * sway(rng), len);
    plan.cone(root, waist, (base, base * 0.55), false, m);
    plan.cone(waist, tip, (base * 0.55, base * 0.15), false, m);
    plan.ellipsoid(
        f.at(at.0, at.1, 0.0),
        f.n,
        (base * 0.7, base * 1.8),
        false,
        m,
    );
}
