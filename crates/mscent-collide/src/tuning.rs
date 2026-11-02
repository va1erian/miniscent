//! Collision-local tolerances. The project-wide plane epsilon lives in
//! `mscent_math::epsilon`; these are the sweep-specific distances built on it.

/// How far a swept sphere is kept off the surface it hit. A sphere resting
/// exactly on a plane would otherwise flip between "touching" and
/// "penetrating" from rounding noise, which shows up as jitter.
pub const SKIN: f64 = 1.0e-3;

/// A point within this distance of a plane counts as lying on it when
/// extracting cell geometry and when testing that a contact is on a face.
pub const ON_PLANE: f64 = 1.0e-6;

/// A pushed body may overlap its mover by this much before it counts as
/// squeezed: several times the skin, since a body pressed between a wall and a
/// mover sits a skin off each.
pub const SQUEEZE_SLACK: f64 = 2.0e-3;

/// Extra reach, beyond the sphere radius, when deciding which BSP subtrees a
/// sweep can touch; covers the skin and embedded (t = 0) contacts.
pub const PRUNE_MARGIN: f64 = 1.0e-2;

/// Edges shorter than this are ignored; their endpoints are vertices anyway.
pub const MIN_EDGE: f64 = 1.0e-7;

/// Polygons below this area are slivers from coincident planes.
pub const MIN_FACE_AREA: f64 = 1.0e-10;

/// Velocities shorter than this are treated as at rest.
pub const MIN_SPEED: f64 = 1.0e-9;

/// Slack when asking whether a clipped velocity still points into a plane.
pub const CLIP_SLACK: f64 = 1.0e-9;
