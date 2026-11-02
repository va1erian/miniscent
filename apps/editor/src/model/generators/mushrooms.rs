//! Mushrooms in a cluster: a leaning stem, a domed cap hollowed beneath, and
//! gills cut into the hollow.

use std::f64::consts::TAU;

use mscent_math::rng::Rng;
use mscent_world::materials::{CAP, STEM};

use super::Plan;
use super::site::{Frame, Site};

const MIN_CAPS: u32 = 3;
const EXTRA_CAPS: u32 = 3;
const GILLS: u32 = 12;

pub(super) fn grow(plan: &mut Plan, f: &Frame, site: &Site, rng: &mut Rng) {
    let reach = 2.2 * site.size;
    let count = MIN_CAPS + rng.below(EXTRA_CAPS);
    for k in 0..count {
        // The first is the biggest, in the middle; the rest crowd round it.
        let (r, a) = if k == 0 {
            (0.0, 0.0)
        } else {
            (reach * rng.range_f64(0.55, 1.0), rng.range_f64(0.0, TAU))
        };
        let scale = site.size
            * if k == 0 {
                1.0
            } else {
                rng.range_f64(0.5, 0.85)
            };
        cap_and_stem(plan, f, (r * a.cos(), r * a.sin()), scale, rng);
    }
}

fn cap_and_stem(plan: &mut Plan, f: &Frame, at: (f64, f64), s: f64, rng: &mut Rng) {
    let (cap, stem) = (CAP as u8, STEM as u8);
    let lean = (
        rng.range_f64(-0.25, 0.25) * s,
        rng.range_f64(-0.25, 0.25) * s,
    );
    let root = f.at(at.0, at.1, -0.3 * s);
    let top = f.at(at.0 + lean.0, at.1 + lean.1, 1.7 * s);
    let dome = top + f.n * (0.15 * s);
    // The dome, then the hollow beneath it that leaves a rim and a stalk's top.
    plan.ellipsoid(dome, f.n, (0.65 * s, 1.3 * s), false, cap);
    plan.ellipsoid(dome - f.n * (0.5 * s), f.n, (0.65 * s, 1.05 * s), true, cap);
    for g in 0..GILLS {
        let a = TAU * f64::from(g) / f64::from(GILLS);
        let out = f.dir(a.cos(), a.sin(), 0.0);
        let inner = dome - f.n * (0.28 * s);
        plan.cone(
            inner,
            inner + out * (1.0 * s) - f.n * (0.1 * s),
            (0.07 * s, 0.07 * s),
            true,
            cap,
        );
    }
    plan.cone(root, top, (0.4 * s, 0.26 * s), false, stem);
    plan.ellipsoid(f.at(at.0, at.1, 0.0), f.n, (0.35 * s, 0.7 * s), false, stem);
}
