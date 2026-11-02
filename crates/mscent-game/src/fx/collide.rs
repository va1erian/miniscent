//! The three collision levels and the per-tick limits that bound their cost.
//!
//! Measured costs (docs/particles.md): `is_solid` 22 ns, a short sweep 222 ns,
//! a long one 24 us. A per-particle trace is therefore out of the question;
//! particles get a point test, and only a bouncing particle that actually
//! entered a wall pays for one surface query to learn where it touched and
//! which way the wall faces.

use super::class::Collide;
use super::world::FxWorld;
use mscent_math::Vec3;
use mscent_math::epsilon::{PARTICLE_NORMAL_PROBE, PARTICLE_REST_SPEED, PLANE_EPSILON_F32};

/// Point tests per tick, over all classes, in units of one BSP test. A test
/// at a point inside a volume costs [`VOXEL_TEST_COST`] of them, and a
/// bounce in a volume [`VOXEL_SWEEP_COST`] more, because a voxel test takes
/// about 8 us against the BSP's 0.1 us (docs/voxels.md). The cost is a
/// compromise: the true ratio would leave one voxel test a tick. With no
/// volumes every test costs 1.
pub const MAX_SOLID_TESTS: u32 = 512;
/// Units one point test inside a volume takes from [`MAX_SOLID_TESTS`].
pub const VOXEL_TEST_COST: u32 = 16;
/// Units a bounce whose step touches a volume takes besides its sweep: about
/// `VOXEL_BISECTIONS + 8` voxel tests that nothing else counts.
pub const VOXEL_SWEEP_COST: u32 = 128;
/// Surface queries (the design's "sweeps") per tick, over all classes. Each
/// is at most `BISECTIONS + 8` extra `is_solid` calls, not counted above.
pub const MAX_SWEEPS: u32 = 32;
/// Halvings of a bounce's step; the contact is within step / 2^20 of the wall.
const BISECTIONS: u32 = 20;
/// The same in a volume, where each halving costs a voxel test: the contact
/// is within step / 2^8 of the rock, a few millimetres for a spark.
const VOXEL_BISECTIONS: u32 = 8;

/// What the tick's collision work has spent so far.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Budget {
    pub solid_tests: u32,
    pub sweeps: u32,
}

impl Budget {
    /// Charges one point test at `p`: dearer inside a volume.
    fn take_test(&mut self, world: FxWorld, p: Vec3) -> bool {
        let cost = if world.in_volume(p) {
            VOXEL_TEST_COST
        } else {
            1
        };
        take(&mut self.solid_tests, MAX_SOLID_TESTS, cost)
    }

    fn take_voxel_sweep(&mut self) -> bool {
        take(&mut self.solid_tests, MAX_SOLID_TESTS, VOXEL_SWEEP_COST)
    }

    fn take_sweep(&mut self) -> bool {
        take(&mut self.sweeps, MAX_SWEEPS, 1)
    }
}

fn take(used: &mut u32, max: u32, cost: u32) -> bool {
    match used.checked_add(cost) {
        Some(total) if total <= max => {
            *used = total;
            true
        }
        _ => false,
    }
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
    world: FxWorld,
    budget: &mut Budget,
    prev: Vec3,
    pos: Vec3,
    vel: Vec3,
) -> Outcome {
    match mode {
        Collide::None => Outcome::Free,
        Collide::Die => {
            if budget.take_test(world, pos) && world.is_solid(pos.to_f64()) {
                Outcome::Dead
            } else {
                Outcome::Free
            }
        }
        Collide::Bounce { keep } => bounce(world, budget, prev, pos, vel, keep),
    }
}

fn bounce(
    world: FxWorld,
    budget: &mut Budget,
    prev: Vec3,
    pos: Vec3,
    vel: Vec3,
    keep: f32,
) -> Outcome {
    if !budget.take_test(world, pos) {
        return Outcome::Dead;
    }
    if !world.is_solid(pos.to_f64()) {
        return Outcome::Free;
    }
    if vel.length() < PARTICLE_REST_SPEED {
        return if world.is_solid(prev.to_f64()) {
            Outcome::Dead
        } else {
            Outcome::Bounced {
                pos: prev,
                vel: Vec3::ZERO,
            }
        };
    }
    if !budget.take_sweep() {
        return Outcome::Dead;
    }
    let steps = if world.touches_volume(prev, pos) {
        if !budget.take_voxel_sweep() {
            return Outcome::Dead;
        }
        VOXEL_BISECTIONS
    } else {
        BISECTIONS
    };
    let Some(contact) = last_empty_point(world, prev, pos, steps) else {
        return Outcome::Dead;
    };
    let normal = surface_normal(world, contact)
        .or_else(|| vel.normalized().map(|d| d * -1.0))
        .unwrap_or(Vec3::ZERO);
    if normal == Vec3::ZERO {
        return Outcome::Dead;
    }
    Outcome::Bounced {
        pos: stand_off(world, contact, normal),
        vel: reflect(vel, normal, keep),
    }
}

/// The surface query a bounce needs, standing in for the design's "short
/// sweep". `mscent_collide::sweep_sphere` would give an exact normal, but it
/// builds its cells in a `Vec` on every call, and the particle tick promises
/// zero allocation. This bisects `prev..pos` with `is_solid`, so the point it
/// returns is empty by the world's own verdict (that is what keeps a bouncing
/// particle out of solid), within `|pos - prev| / 2^BISECTIONS` of the wall.
/// `None` when `prev` is itself solid, as for a particle born in a wall.
/// `steps` is [`BISECTIONS`], or [`VOXEL_BISECTIONS`] where each test is dear.
fn last_empty_point(world: FxWorld, prev: Vec3, pos: Vec3, steps: u32) -> Option<Vec3> {
    if world.is_solid(prev.to_f64()) {
        return None;
    }
    let (mut empty, mut solid) = (prev, pos);
    for _ in 0..steps {
        let mid = empty.lerp(solid, 0.5);
        if world.is_solid(mid.to_f64()) {
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
///
/// A voxel surface within reach gives its own normal instead: its rock is a
/// smooth mesh, where six axis probes would tilt every slope toward a
/// diagonal.
fn surface_normal(world: FxWorld, at: Vec3) -> Option<Vec3> {
    world.voxel_normal(at).or_else(|| probed_normal(world, at))
}

fn probed_normal(world: FxWorld, at: Vec3) -> Option<Vec3> {
    let h = PARTICLE_NORMAL_PROBE;
    let mut away = Vec3::ZERO;
    for axis in [
        Vec3::new(1.0, 0.0, 0.0),
        Vec3::new(0.0, 1.0, 0.0),
        Vec3::new(0.0, 0.0, 1.0),
    ] {
        for sign in [1.0, -1.0] {
            let step = axis * (sign * h);
            if world.is_solid((at + step).to_f64()) {
                away = away - step;
            }
        }
    }
    away.normalized()
}

/// Moves a contact point a plane-epsilon off the surface so the particle does
/// not sit exactly on the plane, unless that spot is solid after all.
fn stand_off(world: FxWorld, contact: Vec3, normal: Vec3) -> Vec3 {
    let moved = contact + normal * PLANE_EPSILON_F32;
    if moved.is_finite() && !world.is_solid(moved.to_f64()) {
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
