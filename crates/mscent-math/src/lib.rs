//! Vectors, matrices, planes, AABB and the single epsilon policy.
//!
//! Written against `core`-style APIs only (no I/O, no threads) so the crate
//! could become `no_std + alloc` later.

pub mod aabb;
pub mod epsilon;
pub mod mat;
pub mod plane;
pub mod polygon;
pub mod quat;
pub mod rng;
pub mod vec;

pub use aabb::{Aabb, BoxSide};
pub use mat::{Mat3, Mat4};
pub use plane::{DPlane, Plane, PolySide, Side};
pub use polygon::{Polygon, Split};
pub use quat::Quat;
pub use vec::{Vec2, Vec3, Vec3d, Vec4};
