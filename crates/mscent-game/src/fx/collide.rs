//! The three collision levels and the per-tick limits that bound their cost.
//!
//! Measured costs (docs/particles.md): `is_solid` 22 ns, a short sweep 222 ns,
//! a long one 24 us. A per-particle trace is therefore out of the question;
//! particles get a point test, and only a bouncing particle that actually
//! entered a wall pays for one surface query to learn where it touched and
//! which way the wall faces.

use super::class::Collide;
use mscent_math::Vec3;
use mscent_math::epsilon::{PARTICLE_NORMAL_PROBE, PLANE_EPSILON_F32};
use mscent_world::Bsp;

/// `is_solid` calls per tick, over all classes.
pub const MAX_SOLID_TESTS: u32 = 512;
/// Surface queries (the design's "sweeps") per tick, over all classes. Each
/// is at most `BISECTIONS + 8` extra `is_solid` calls, not counted above.
pub const MAX_SWEEPS: u32 = 32;
/// Halvings of a bounce's step; the contact is within step / 2^20 of the wall.
const BISECTIONS: u32 = 20;

/// What the tick's collision work has spent so far.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Budget {
    pub solid_tests: u32,
    pub sweeps: u32,
}

impl Budget {
    fn take_test(&mut self) -> bool {
        take(&mut self.solid_tests, MAX_SOLID_TESTS)
    }

    fn take_sweep(&mut self) -> bool {
        take(&mut self.sweeps, MAX_SWEEPS)
    }
}

fn take(used: &mut u32, max: u32) -> bool {
    if *used >= max {
        return false;
    }
    *used += 1;
    true
}

/// What happened to a particle that moved from `prev` to `pos`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Outcome {
    /// Leave it where the integrator put it.
    Free,
    Dead,
    /// Bounced: new position (outside solid) and velocity.
    Bounced {
        pos: Vec3,
        vel: Vec3,
    },
}

/// Applies `mode` to a particle that moved from `prev` to `pos`.
///
/// Over the test limit a `Die` particle skips the test and passes through.
/// A `Bounce` particle dies instead: skipping would leave it free to end the
/// tick inside solid, and that is the one thing a bouncing class promises
/// never to do. Callers run Bounce classes first so they are the last to
/// meet the limit.
pub fn resolve(
    mode: Collide,
    bsp: &Bsp,
    budget: &mut Budget,
    prev: Vec3,
    pos: Vec3,
    vel: Vec3,
) -> Outcome {
    match mode {
        Collide::None => Outcome::Free,
        Collide::Die => {
            if budget.take_test() && bsp.is_solid(pos.to_f64()) {
                Outcome::Dead
            } else {
                Outcome::Free
            }
        }
        Collide::Bounce { keep } => bounce(bsp, budget, prev, pos, vel, keep),
    }
}

fn bounce(bsp: &Bsp, budget: &mut Budget, prev: Vec3, pos: Vec3, vel: Vec3, keep: f32) -> Outcome {
    if !budget.take_test() {
        return Outcome::Dead;
    }
    if !bsp.is_solid(pos.to_f64()) {
        return Outcome::Free;
    }
    if !budget.take_sweep() {
        return Outcome::Dead;
    }
    let Some(contact) = last_empty_point(bsp, prev, pos) else {
        return Outcome::Dead;
    };
    let normal = surface_normal(bsp, contact)
        .or_else(|| vel.normalized().map(|d| d * -1.0))
        .unwrap_or(Vec3::ZERO);
    if normal == Vec3::ZERO {
        return Outcome::Dead;
    }
    Outcome::Bounced {
        pos: stand_off(bsp, contact, normal),
        vel: reflect(vel, normal, keep),
    }
}

/// The surface query a bounce needs, standing in for the design's "short
/// sweep". `mscent_collide::sweep_sphere` would give an exact normal, but it
/// builds its cells in a `Vec` on every call, and the particle tick promises
/// zero allocation. This bisects `prev..pos` with `is_solid`, so the point it
/// returns is empty by the BSP's own verdict (that is what keeps a bouncing
/// particle out of solid), within `|pos - prev| / 2^BISECTIONS` of the wall.
/// `None` when `prev` is itself solid, as for a particle born in a wall.
fn last_empty_point(bsp: &Bsp, prev: Vec3, pos: Vec3) -> Option<Vec3> {
    if bsp.is_solid(prev.to_f64()) {
        return None;
    }
    let (mut empty, mut solid) = (prev, pos);
    for _ in 0..BISECTIONS {
        let mid = empty.lerp(solid, 0.5);
        if bsp.is_solid(mid.to_f64()) {
            solid = mid;
        } else {
            empty = mid;
        }
    }
    Some(empty)
}

/// Unit normal of the surface next to `at`: away from whichever of the six
/// axis neighbours are solid. Exact for axis-aligned walls, and for a 45
/// degree wall; coarser for other slopes, which a spark's bounce does not
/// show. `None` when no neighbour is solid or they cancel.
fn surface_normal(bsp: &Bsp, at: Vec3) -> Option<Vec3> {
    let h = PARTICLE_NORMAL_PROBE;
    let mut away = Vec3::ZERO;
    for axis in [
        Vec3::new(1.0, 0.0, 0.0),
        Vec3::new(0.0, 1.0, 0.0),
        Vec3::new(0.0, 0.0, 1.0),
    ] {
        for sign in [1.0, -1.0] {
            let step = axis * (sign * h);
            if bsp.is_solid((at + step).to_f64()) {
                away = away - step;
            }
        }
    }
    away.normalized()
}

/// Moves a contact point a plane-epsilon off the surface so the particle does
/// not sit exactly on the plane, unless that spot is solid after all.
fn stand_off(bsp: &Bsp, contact: Vec3, normal: Vec3) -> Vec3 {
    let moved = contact + normal * PLANE_EPSILON_F32;
    if moved.is_finite() && !bsp.is_solid(moved.to_f64()) {
        moved
    } else {
        contact
    }
}

/// Mirrors the velocity about the surface and scales it by `keep`. A velocity
/// already leaving the surface is only scaled, never turned back into it.
fn reflect(vel: Vec3, normal: Vec3, keep: f32) -> Vec3 {
    let keep = if keep.is_finite() {
        keep.clamp(0.0, 1.0)
    } else {
        0.0
    };
    let into = vel.dot(normal);
    let v = if into < 0.0 {
        vel - normal * (2.0 * into)
    } else {
        vel
    };
    v * keep
}
