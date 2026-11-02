//! Every numeric tolerance in the project lives here, so there is one place to
//! reason about robustness. Do not write a literal tolerance anywhere else.

/// Distance within which an `f64` point counts as lying on a plane. CSG
/// classification and polygon splitting use this. It is far below the snap
/// grid, so snapped geometry never lands in the ambiguous band by accident.
pub const PLANE_EPSILON_F64: f64 = 1.0e-5;

/// Distance within which an `f32` point counts as lying on a plane. Runtime
/// code (collision, rendering) works on `f32` data stored from the `f64`
/// build, so this is looser than the build tolerance.
pub const PLANE_EPSILON_F32: f32 = 1.0e-3;

/// Edge length of the grid that brush planes are snapped to before the CSG
/// build, in world units. A power of two so snapping is exact in binary
/// floating point.
pub const SNAP_GRID: f64 = 0.125;

/// Vectors shorter than this are treated as zero when normalizing, so a
/// degenerate input yields `None` instead of NaN or infinity.
pub const MIN_LENGTH_F64: f64 = 1.0e-9;

/// `f32` counterpart of [`MIN_LENGTH_F64`].
pub const MIN_LENGTH_F32: f32 = 1.0e-6;

/// Polygons whose area is below this (in squared world units) are discarded
/// as slivers after splitting.
pub const MIN_POLYGON_AREA: f64 = 1.0e-8;

/// How far off a wall to look for the leaf a surface lookup starts from. Past
/// the collision skin, so a shot's end point is on the empty side.
pub const SURFACE_PROBE: f32 = 0.05;

/// Distance from a face's plane within which a shot's end point counts as on
/// it: the probe distance again, since the end point may be off by that much.
pub const FACE_MATCH_DISTANCE: f32 = 0.05;

/// Least cosine between a shot's normal and a face's for them to be the same
/// surface. Normals come from the same plane, so this only absorbs rounding.
pub const FACE_MATCH_COS: f32 = 0.99;

/// Largest allowed deviation of a unit quaternion's squared norm from one
/// before tests consider it unnormalized.
pub const UNIT_NORM_TOLERANCE: f32 = 1.0e-4;

/// Distance from a particle's contact point at which the six axis probes look
/// for solid when estimating a surface normal. Larger than the bisection
/// error of a bounce, smaller than any wall the editor snaps to.
pub const PARTICLE_NORMAL_PROBE: f32 = 5.0e-2;
