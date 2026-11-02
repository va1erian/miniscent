//! Trees by space colonization: attractor points fill a crown, branches grow
//! toward the ones nearest them until they are reached, and the pipe model sets
//! each branch's thickness from the twigs it carries. The trunk flares into the
//! ground through buttress roots.

use mscent_world::materials::BARK;
use std::f64::consts::TAU;

use mscent_math::Vec3d;
use mscent_math::rng::Rng;

use super::Plan;
use super::site::{Frame, Site};

/// Attractor points in the crown.
const ATTRACTORS: usize = 140;
/// Most branch points. Growth stops here whatever is left to reach.
const MAX_NODES: usize = 420;
/// Rounds of growth.
const ROUNDS: usize = 70;
/// An attractor pulls on the nearest node within this many steps.
const INFLUENCE: f64 = 9.0;
/// A node this close to an attractor, in steps, has reached it.
const REACHED: f64 = 1.5;
/// The pipe model's exponent: a branch's cross section follows its twigs' sum
/// of this power. Trees measure about 2 to 3; the high end keeps a trunk
/// from swelling with every twig.
const PIPE: f64 = 3.0;
/// Buttress roots round the trunk's foot.
const ROOTS: u32 = 6;

struct Node {
    at: Vec3d,
    parent: Option<usize>,
    radius: f64,
}

pub(super) fn grow(plan: &mut Plan, f: &Frame, site: &Site, rng: &mut Rng) {
    let s = site.size;
    let step = (0.5 * s).max(site.cell * 2.0);
    let crown = f.at(0.0, 0.0, 3.9 * s);
    let mut nodes = trunk(f, s, step);
    let mut attractors = crown_points(f, crown, s, rng);
    colonize(&mut nodes, &mut attractors, step, f);
    let tip = (0.05 * s).max(site.cell);
    thicken(&mut nodes, tip, step);
    let wood = BARK as u8;
    for node in &nodes {
        if let Some(p) = node.parent {
            let up = &nodes[p];
            plan.cone(up.at, node.at, (up.radius, node.radius), false, wood);
        }
    }
    flare(plan, f, nodes.first().map_or(tip, |n| n.radius), wood);
}

/// The straight trunk from the foot up to where the crown begins.
fn trunk(f: &Frame, s: f64, step: f64) -> Vec<Node> {
    let height = 1.8 * s;
    let count = (height / step).ceil() as usize;
    (0..=count)
        .map(|i| Node {
            at: f.at(
                0.0,
                0.0,
                -0.4 * s + (height + 0.4 * s) * i as f64 / count as f64,
            ),
            parent: i.checked_sub(1),
            radius: 0.0,
        })
        .collect()
}

/// Random points inside the crown's ellipsoid, wider than tall.
fn crown_points(f: &Frame, centre: Vec3d, s: f64, rng: &mut Rng) -> Vec<Vec3d> {
    let (wide, tall) = (2.8 * s, 2.2 * s);
    let mut out = Vec::with_capacity(ATTRACTORS);
    while out.len() < ATTRACTORS {
        let q = Vec3d::new(
            rng.range_f64(-1.0, 1.0),
            rng.range_f64(-1.0, 1.0),
            rng.range_f64(-1.0, 1.0),
        );
        if q.length_squared() <= 1.0 {
            out.push(centre + f.dir(q.x * wide, q.y * wide, q.z * tall));
        }
    }
    out
}

/// Grows branches toward the attractors until they are all reached, or growth
/// stalls or runs out of nodes.
fn colonize(nodes: &mut Vec<Node>, attractors: &mut Vec<Vec3d>, step: f64, f: &Frame) {
    for _ in 0..ROUNDS {
        if attractors.is_empty() || nodes.len() >= MAX_NODES {
            return;
        }
        let mut pull = vec![Vec3d::ZERO; nodes.len()];
        let mut pulled = vec![false; nodes.len()];
        for a in attractors.iter() {
            let near = nodes
                .iter()
                .enumerate()
                .map(|(i, n)| (i, (*a - n.at).length()))
                .filter(|&(_, d)| d < INFLUENCE * step)
                .min_by(|x, y| x.1.total_cmp(&y.1));
            if let Some((i, d)) = near
                && let Some(dir) = (*a - nodes[i].at).normalized()
            {
                pull[i] = pull[i] + dir * (1.0 / (1.0 + d));
                pulled[i] = true;
            }
        }
        let before = nodes.len();
        for i in 0..before {
            // A branch also leans away from the surface, so none dives into it.
            let Some(dir) = (pull[i] + f.n * 0.02).normalized().filter(|_| pulled[i]) else {
                continue;
            };
            let at = nodes[i].at + dir * step;
            if nodes.len() < MAX_NODES {
                nodes.push(Node {
                    at,
                    parent: Some(i),
                    radius: 0.0,
                });
            }
        }
        if nodes.len() == before {
            return;
        }
        attractors.retain(|a| nodes.iter().all(|n| (*a - n.at).length() > REACHED * step));
    }
}

/// Radii by the pipe model: a twig is as thick as `tip`, and a branch carries
/// the sum of its twigs' cross sections, plus a little taper along a bare run.
fn thicken(nodes: &mut [Node], tip: f64, step: f64) {
    let mut carried = vec![0.0_f64; nodes.len()];
    for i in (0..nodes.len()).rev() {
        let r = if carried[i] > 0.0 {
            carried[i].powf(1.0 / PIPE) + 0.03 * step
        } else {
            tip
        };
        nodes[i].radius = r;
        if let Some(p) = nodes[i].parent {
            carried[p] += r.powf(PIPE);
        }
    }
}

/// The foot: roots running out and down from the trunk, and a swelling.
fn flare(plan: &mut Plan, f: &Frame, trunk: f64, wood: u8) {
    plan.ellipsoid(
        f.at(0.0, 0.0, 0.0),
        f.n,
        (trunk * 0.9, trunk * 1.5),
        false,
        wood,
    );
    for k in 0..ROOTS {
        let a = TAU * f64::from(k) / f64::from(ROOTS) + 0.3;
        let out = f.dir(a.cos(), a.sin(), 0.0);
        let from = f.at(0.0, 0.0, trunk * 0.9) + out * (trunk * 0.4);
        let to = f.at(0.0, 0.0, -trunk * 0.5) + out * (trunk * 2.6);
        plan.cone(from, to, (trunk * 0.55, trunk * 0.22), false, wood);
    }
}
