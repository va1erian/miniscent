//! Brushes, CSG to BSP, portals, lightmap bake and the level format.
//!
//! # Public API (M2.1 to M2.3)
//!
//! Model: the world starts **solid**. Brushes are convex, applied in ascending
//! `order`; a [`Op::Subtract`] brush carves empty space, an [`Op::Add`] brush
//! fills solid again. The last brush covering a point decides its kind.
//!
//! * [`Brush`] (`brush.rs`): convex plane set, planes point **outward**
//!   (`normal . p <= dist` is inside). All constructors validate and return
//!   `Result<Brush, BrushError>`; hostile input never panics.
//!   - `Brush::cuboid(min, max, op, order)` (corners snapped to
//!     [`mscent_math::epsilon::SNAP_GRID`]), `Brush::new(&[DPlane], op, order)`,
//!     `Brush::from_point_triples`, `Brush::from_polygons`.
//!   - `planes()`, `faces()` (`faces()[i]` lies on `planes()[i]`, wound
//!     counter-clockwise from outside), `op()`, `order()`, `contains(Vec3d)`,
//!     `bounds()`, `flipped_op()`.
//!   - Limits: [`brush::MAX_PLANES`], coordinates within `polytope::MAX_COORD`.
//! * [`Bsp`] (`bsp.rs`): `Node { plane: DPlane, front, back }` and
//!   `Leaf { kind: LeafKind::{Solid, Empty} }`, stored in flat arrays and
//!   addressed by [`NodeId`] / [`LeafId`] (`u32` newtypes; a leaf's id is its
//!   index). [`Child`] is `Node(NodeId) | Leaf(LeafId)`. Front is
//!   `normal . p - dist > 0`; points exactly on a plane go back.
//!   - Queries: `leaf_at(Vec3d)`, `leaf_at_f32(Vec3)`, `is_solid(Vec3d)`,
//!     `leaf_kind(LeafId)`, `classify_aabb(&Aabb)` and
//!     `classify_sphere(center, radius)` returning [`Region`]
//!     (`Solid | Empty | Mixed` over all leaves touched), `depth()`.
//!   - Accessors: `nodes()`, `leaves()`, `root()`, `node(id)`, `leaf(id)`.
//!   - `Bsp::from_parts(nodes, leaves, root)` validates untrusted trees
//!     (indices, true tree shape, depth <= [`bsp::MAX_DEPTH`]).
//! * [`compile`] (`csg.rs`): `compile(&[Brush]) -> Result<Bsp, CsgError>`.
//!   Planes are `f64`; call `Node::plane_f32()` for runtime use.
//!
//! Leaves are convex cells, but adjacent leaves of the same kind are not
//! merged; later stages (polygon extraction, portals) must not assume
//! maximal cells.

pub mod brush;
pub mod bsp;
pub mod csg;
pub mod entity;
pub mod faces;
pub mod format;
mod format_wire;
pub mod r#gen;
pub mod lightmap;
pub mod polytope;
pub mod portals;

pub use brush::{Brush, BrushError, Op};
pub use bsp::{Bsp, BspError, Child, Leaf, LeafId, LeafKind, Node, NodeId, Region};
pub use csg::{CsgError, compile};
pub use entity::{Entity, Level};
pub use faces::{Face, extract_faces};
pub use format::FormatError;
pub use r#gen::{GenParams, generate, room_centers};
pub use portals::{Portal, Portals};

#[cfg(test)]
mod brush_tests;
#[cfg(test)]
mod bsp_tests;
#[cfg(test)]
mod csg_tests;
#[cfg(test)]
mod format_tests;
#[cfg(test)]
mod gen_tests;
#[cfg(test)]
mod portals_tests;
