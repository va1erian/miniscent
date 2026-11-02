//! Coral: branches that fork as they climb, each thinner than the last, with a
//! random lean and a pull away from the surface.

use std::f64::consts::TAU;

use mscent_math::Vec3d;
use mscent_math::rng::Rng;

use super::Plan;
use super::site::{Frame, Site};

const MIN_COLONIES: u32 = 3;
const EXTRA_COLONIES: u32 = 2;
/// Forks deep a colony goes.
const DEPTH: u32 = 4;
/// Branches that leave each fork.
const FORKS: usize = 2;

/// A branch still to be grown.
struct Twig {
    from: Vec3d,
    dir: Vec3d,
    radius: f64,
    depth: u32,
}

pub(super) fn grow(plan: &mut Plan, f: &Frame, site: &Site, rng: &mut Rng) {
    let reach = 1.6 * site.size;
    let colonies = MIN_COLONIES + rng.below(EXTRA_COLONIES);
    for _ in 0..colonies {
        let (r, a) = (reach * rng.next_f64().sqrt(), rng.range_f64(0.0, TAU));
        let from = f.at(r * a.cos(), r * a.sin(), -0.3 * site.size);
        colony(plan, f, site, from, rng);
    }
}

/// Grows one colony depth first, so the plan stays bounded by `FORKS^DEPTH`.
fn colony(plan: &mut Plan, f: &Frame, site: &Site, from: Vec3d, rng: &mut Rng) {
    let mut stack = vec![Twig {
        from,
        dir: f.n,
        radius: 0.5 * site.size,
        depth: DEPTH,
    }];
    while let Some(t) = stack.pop() {
        let to = t.from + t.dir * (t.radius * rng.range_f64(2.6, 3.8));
        let end = t.radius * 0.78;
        plan.cone(t.from, to, (t.radius, end), false, site.material);
        if t.depth == 0 {
            plan.sphere(to, end * 1.25, false, site.material);
            continue;
        }
        for _ in 0..FORKS {
            let turn = f.dir(
                rng.range_f64(-0.8, 0.8),
                rng.range_f64(-0.8, 0.8),
                rng.range_f64(0.1, 0.7),
            );
            let dir = (t.dir + turn).normalized().unwrap_or(f.n);
            stack.push(Twig {
                from: to,
                dir,
                radius: end,
                depth: t.depth - 1,
            });
        }
    }
}
