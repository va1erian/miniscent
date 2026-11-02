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

/// How far, in cells, a voxel surface sinks behind the brush faces it meets at
/// the border of a volume, so the two never fight and never leave a gap.
pub const VOXEL_SEAM_BIAS_CELLS: f64 = 1.0 / 16.0;

/// Vectors shorter than this are treated as zero when normalizing, so a
/// degenerate input yields `None` instead of NaN or infinity.
pub const MIN_LENGTH_F64: f64 = 1.0e-9;

/// `f32` counterpart of [`MIN_LENGTH_F64`].
pub const MIN_LENGTH_F32: f32 = 1.0e-6;

/// Polygons whose area is below this (in squared world units) are discarded
/// as slivers after splitting.
pub const MIN_POLYGON_AREA: f64 = 1.0e-8;

/// Largest allowed deviation of a unit quaternion's squared norm from one
/// before tests consider it unnormalized.
pub const UNIT_NORM_TOLERANCE: f32 = 1.0e-4;
