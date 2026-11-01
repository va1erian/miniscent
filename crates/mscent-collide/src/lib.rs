//! Trace and sweep against the BSP and entities.
//!
//! # Public API (M4.1 to M4.3, M4.5, M4.6)
//!
//! * [`sweep_sphere`] (`sweep.rs`): `sweep_sphere(&Bsp, from, to, radius) ->`
//!   [`Trace`]. Exact first contact with the solid cells: faces pushed out by
//!   the radius, edges as cylinders and vertices as spheres (so the sphere
//!   rounds convex corners instead of catching on a square one). After a hit
//!   the centre is held a small skin off the surface.
//! * [`slide_move`] (`slide.rs`): `slide_move(&Bsp, pos, vel, dt, radius) ->`
//!   [`SlideResult`]. At most [`MAX_BUMPS`] deflections; two-plane creases
//!   slide along the crease, three planes stop.
//! * [`sweep_sphere_sphere`], [`sweep_spheres`] (`spheres.rs`): analytic
//!   entity sweeps returning [`SphereHit`].
//! * [`Body`], [`FixedStepper`], [`Simulation`], [`interpolate`] (`step.rs`):
//!   120 Hz fixed step driven by integer nanoseconds, with interpolation.
//!
//! Solid-cell geometry is rebuilt from the BSP planes on demand for the
//! leaves a sweep can reach, so the library holds no cache and needs no
//! build step beyond the compiled [`mscent_world::Bsp`].

mod cell;
mod prim;
pub mod slide;
pub mod spheres;
pub mod step;
pub mod sweep;
pub mod tuning;

pub use slide::{MAX_BUMPS, SlideResult, slide_move};
pub use spheres::{SphereHit, sweep_sphere_sphere, sweep_spheres};
pub use step::{
    Body, FixedStepper, MAX_STEPS_PER_FRAME, STEP_DT, STEP_HZ, Simulation, interpolate,
};
pub use sweep::{Trace, sweep_sphere};

#[cfg(test)]
mod slide_tests;
#[cfg(test)]
mod step_tests;
#[cfg(test)]
mod sweep_tests;
#[cfg(test)]
mod test_util;
